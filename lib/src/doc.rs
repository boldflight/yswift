use crate::array::YrsArray;
use crate::error::CodingError;
use crate::map::YrsMap;
use crate::text::YrsText;
use crate::xml::YrsXmlNode;
use crate::xml::YrsXmlResolvedPosition;
use crate::transaction::YrsTransaction;
use std::sync::Arc;
use std::{borrow::Borrow, cell::RefCell};
use yrs::{updates::decoder::Decode, ArrayRef, Doc, OffsetKind, Options, StateVector, Transact, Origin};
use yrs::XmlOut;
use yrs::{MapRef, ReadTxn};
use yrs::branch::Branch;
use crate::undo::YrsUndoManager;
use crate::UniffiCustomTypeConverter;

pub(crate) struct YrsDoc(RefCell<Doc>);

unsafe impl Send for YrsDoc {}
unsafe impl Sync for YrsDoc {}

impl YrsDoc {
    pub(crate) fn new() -> Self {
        let mut options = Options::default();
        options.offset_kind = OffsetKind::Utf16;
        let doc = yrs::Doc::with_options(options);

        Self(RefCell::from(doc))
    }

    pub(crate) fn encode_diff_v1(
        &self,
        transaction: &YrsTransaction,
        state_vector: Vec<u8>,
    ) -> Result<Vec<u8>, CodingError> {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        StateVector::decode_v1(state_vector.borrow())
            .map_err(|_e| CodingError::DecodingError)
            .map(|sv| tx.encode_diff_v1(&sv))
    }

    pub(crate) fn get_text(&self, name: String) -> Arc<YrsText> {
        let text_ref = self.0.borrow().get_or_insert_text(name.as_str());
        Arc::new(YrsText::new(text_ref, self.0.borrow().clone()))
    }

    pub(crate) fn get_xml_fragment(&self, name: String) -> Arc<YrsXmlNode> {
        let fragment = self.0.borrow().get_or_insert_xml_fragment(name.as_str());
        Arc::new(YrsXmlNode::from_fragment(fragment))
    }

    pub(crate) fn resolve_xml_relative_position(
        &self,
        transaction: &YrsTransaction,
        encoded: Vec<u8>,
    ) -> Result<Option<YrsXmlResolvedPosition>, CodingError> {
        let (position, association) = crate::xml::decode_xml_relative_position(&encoded)?;
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let Some(offset) = position.get_offset(tx) else { return Ok(None) };
        if offset.branch.is_deleted() { return Ok(None) }
        let Ok(node) = XmlOut::try_from(offset.branch) else { return Ok(None) };
        Ok(Some(YrsXmlResolvedPosition {
            node: Arc::new(YrsXmlNode::from_node(node)),
            index: offset.index,
            association,
        }))
    }

    pub(crate) fn get_array(&self, name: String) -> Arc<YrsArray> {
        let array_ref: ArrayRef = self.0.borrow().get_or_insert_array(name.as_str()).into();
        Arc::new(YrsArray::new(array_ref, self.0.borrow().clone()))
    }

    pub(crate) fn get_map(&self, name: String) -> Arc<YrsMap> {
        let map_ref: MapRef = self.0.borrow().get_or_insert_map(name.as_str()).into();
        Arc::new(YrsMap::new(map_ref, self.0.borrow().clone()))
    }

    pub(crate) fn transact<'doc>(&self, origin: Option<YrsOrigin>) -> Arc<YrsTransaction> {
        let tx = self.0.borrow();
        let tx = if let Some(origin) = origin {
            tx.transact_mut_with(origin)
        } else {
            tx.transact_mut()
        };
        Arc::from(YrsTransaction::from(tx))
    }

    pub(crate) fn undo_manager(&self, tracked_refs: Vec<YrsCollectionPtr>) -> Arc<YrsUndoManager> {
        let doc = &*self.0.borrow();
        let mut undo_manager = yrs::undo::UndoManager::new();
        for reference in tracked_refs {
            undo_manager.expand_scope(doc, &reference);
        }
        Arc::new(YrsUndoManager::new(undo_manager, doc.clone()))
    }
}

#[derive(Clone)]
pub(crate) struct YrsOrigin(Arc<[u8]>);

impl From<Origin> for YrsOrigin {
    fn from(value: Origin) -> Self {
        YrsOrigin(Arc::from(value.as_ref()))
    }
}

impl Into<Origin> for YrsOrigin {
    fn into(self) -> Origin {
        Origin::from(self.0.as_ref())
    }
}

impl UniffiCustomTypeConverter for YrsOrigin {
    type Builtin = Vec<u8>;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> where Self: Sized {
        Ok(YrsOrigin(val.into()))
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.0.to_vec()
    }
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub(crate) struct YrsCollectionPtr(*const Branch);

unsafe impl Send for YrsCollectionPtr { }
unsafe impl Sync for YrsCollectionPtr { }

impl AsRef<Branch> for YrsCollectionPtr {
    #[inline]
    fn as_ref(&self) -> &Branch {
        unsafe { self.0.as_ref() }.unwrap()
    }
}

impl<'a> From<&'a Branch> for YrsCollectionPtr {
    #[inline]
    fn from(value: &'a Branch) -> Self {
        let ptr = value as *const Branch;
        YrsCollectionPtr(ptr)
    }
}

impl UniffiCustomTypeConverter for YrsCollectionPtr {
    type Builtin = u64;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> where Self: Sized {
        let ptr = val as usize as *const Branch;
        Ok(YrsCollectionPtr(ptr))
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.0 as usize as u64
    }
}
