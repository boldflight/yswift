use crate::array::YrsArray;
use crate::error::CodingError;
use crate::map::YrsMap;
use crate::text::YrsText;
use crate::xml::YrsXmlNode;
use crate::xml::YrsXmlResolvedPosition;
use crate::awareness::YrsAwareness;
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

    pub(crate) fn with_options(guid: String, retain_deleted_items: bool) -> Self {
        let mut options = Options::default();
        options.offset_kind = OffsetKind::Utf16;
        options.guid = guid.into();
        options.skip_gc = retain_deleted_items;
        Self(RefCell::from(Doc::with_options(options)))
    }

    pub(crate) fn guid(&self) -> String { self.0.borrow().guid().to_string() }

    pub(crate) fn validate_undo_history(&self, tracked_refs: Vec<YrsCollectionPtr>, json: String, scope_token: String) -> Result<(), crate::undo::YrsUndoError> {
        YrsUndoManager::validate_history(&self.0.borrow(), &tracked_refs, &json, &scope_token).map(|_| ())
    }

    pub(crate) fn restore_undo_manager(&self, tracked_refs: Vec<YrsCollectionPtr>, json: String, scope_token: String, metadata: Vec<u64>) -> Result<Arc<YrsUndoManager>, crate::undo::YrsUndoError> {
        YrsUndoManager::restore(self.0.borrow().clone(), tracked_refs, json, scope_token, metadata).map(Arc::new)
    }

    /// Read-only exact format-event observer supplied by the consumed Yrs fork.
    pub(crate) fn inspect_format_event(&self, transaction: &YrsTransaction, update: Vec<u8>, attribute: String, targets_json: String, max_steps: u32) -> Result<String, CodingError> {
        use yrs::branch::BranchID;
        use yrs::format_event::{FormatEvent, FormatTarget};
        use yrs::block::ClientID;
        let input: serde_json::Value = serde_json::from_str(&targets_json).map_err(|_| CodingError::DecodingError)?;
        let object = input.as_object().ok_or(CodingError::InvalidOperation)?;
        if object.len() != 1 { return Err(CodingError::InvalidOperation) }
        let runs = input["runs"].as_array().ok_or(CodingError::InvalidOperation)?;
        let identity = |value: &serde_json::Value| -> Result<yrs::ID, CodingError> {
            let fields = value.as_object().ok_or(CodingError::InvalidOperation)?;
            if fields.len() != 2 { return Err(CodingError::InvalidOperation) }
            let client = value["client"].as_u64().filter(|n| *n <= (1u64 << 53) - 1).ok_or(CodingError::InvalidOperation)?;
            let clock = value["clock"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(CodingError::InvalidOperation)?;
            Ok(yrs::ID::new(ClientID::new(client), clock))
        };
        let mut targets = Vec::new();
        for run in runs {
            if run.as_object().map_or(true, |r| r.len() != 3) { return Err(CodingError::InvalidOperation) }
            let item = identity(&run["item"])?;
            let branch = identity(&run["type"])?;
            let count = run["count"].as_u64().filter(|n| *n > 0 && *n <= 131072).ok_or(CodingError::InvalidOperation)?;
            if u64::from(item.clock) + count > u64::from(u32::MAX) + 1 || targets.len() as u64 + count > 131072 { return Err(CodingError::InvalidOperation) }
            for offset in 0..count {
                targets.push(FormatTarget { branch: BranchID::Nested(branch), item: yrs::ID::new(item.client, item.clock + offset as u32) });
            }
        }
        let tx = transaction.transaction();
        let tx = tx.as_ref().ok_or(CodingError::InvalidOperation)?;
        let event = FormatEvent::inspect(tx, &update, &attribute, &targets, max_steps as usize).map_err(|_| CodingError::InvalidOperation)?;
        let id = |id: yrs::ID| serde_json::json!({"client":id.client.get(),"clock":id.clock});
        let state = |state: yrs::format_event::FormatState| serde_json::json!({"marker":state.marker.map(&id),"value":state.value});
        let changes: Vec<_> = event.changes.into_iter().map(|change| {
            let BranchID::Nested(branch) = change.target.branch else { unreachable!() };
            serde_json::json!({"unit":{"type":id(branch),"item":id(change.target.item)},"before":state(change.before),"after":state(change.after)})
        }).collect();
        Ok(serde_json::json!({"inserted_markers":event.inserted.into_iter().map(&id).collect::<Vec<_>>(),
            "deleted_markers":event.deleted.into_iter().map(&id).collect::<Vec<_>>(),"changes":changes}).to_string())
    }

    /// Read-only direct redone edges from the maintained retained-items owner.
    pub(crate) fn inspect_restored_text_items(&self, transaction: &YrsTransaction, state_vector: Vec<u8>, root_name: String, max_steps: u32) -> Result<String, CodingError> {
        use yrs::branch::BranchID;
        if state_vector.len() > 8*1024*1024 || root_name.is_empty() || max_steps == 0 || max_steps > 1_000_000 { return Err(CodingError::InvalidOperation) }
        let before = StateVector::decode_v1(&state_vector).map_err(|_|CodingError::DecodingError)?;
        let tx=transaction.transaction();let tx=tx.as_ref().ok_or(CodingError::InvalidOperation)?;
        let scope=BranchID::Root(root_name.into());
        let edges=yrs::text_items::observe_restored(tx,&scope,&before,max_steps as usize).map_err(|_|CodingError::InvalidOperation)?;
        let id=|id:yrs::ID|serde_json::json!({"client":id.client.get(),"clock":id.clock});
        let mut copies=Vec::new();
        for edge in edges {
            let BranchID::Nested(source_type)=edge.source_type else{return Err(CodingError::InvalidOperation)};
            let BranchID::Nested(destination_type)=edge.destination_type else{return Err(CodingError::InvalidOperation)};
            copies.push(serde_json::json!({"source":{"runs":[{"type":id(source_type),"item":id(edge.source.item),"count":edge.source.count}]},
                "destination":{"runs":[{"type":id(destination_type),"item":id(edge.destination.item),"count":edge.destination.count}]}}));
        }
        Ok(serde_json::json!({"copies":copies}).to_string())
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

    pub(crate) fn make_awareness(&self) -> Arc<YrsAwareness> {
        Arc::new(YrsAwareness::new(self.0.borrow().clone()))
    }

    pub(crate) fn resolve_xml_relative_position(
        &self,
        transaction: &YrsTransaction,
        encoded: Vec<u8>,
    ) -> Result<Option<YrsXmlResolvedPosition>, CodingError> {
        let (position, association) = crate::relative_position::decode_relative_position(&encoded)?;
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
        let manager = YrsUndoManager::new(yrs::undo::UndoManager::new(), doc.clone());
        for reference in tracked_refs { manager.add_scope(reference); }
        Arc::new(manager)
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
