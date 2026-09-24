use crate::attrs::YrsAttrs;
use crate::doc::YrsCollectionPtr;
use crate::error::CodingError;
use crate::relative_position::{association_bytes, decode_relative_position};
use crate::transaction::YrsTransaction;
use std::sync::Arc;
use yrs::branch::{BranchID, BranchPtr};
use yrs::types::text::YChange;
use yrs::Out as Value;
use yrs::updates::encoder::Encode;
use yrs::{Any, Assoc, GetString, Map, MapRef, StickyIndex, Text, XmlElementPrelim, XmlFragment, XmlFragmentRef, XmlOut as XmlNode, XmlTextPrelim};

/// A single reference type keeps nested XML nodes integrated in their Yrs document.
pub(crate) struct YrsXmlNode(BranchID);

pub(crate) struct YrsXmlAttribute { pub(crate) key: String, pub(crate) value_json: String }
pub(crate) struct YrsXmlTextRun { pub(crate) text: String, pub(crate) attributes_json: String }
pub(crate) struct YrsXmlResolvedPosition {
    pub(crate) node: Arc<YrsXmlNode>,
    pub(crate) index: u32,
    pub(crate) association: i32,
}

impl YrsXmlNode {
    pub(crate) fn from_fragment(fragment: XmlFragmentRef) -> Self { Self(XmlNode::Fragment(fragment).id()) }
    pub(crate) fn from_node(node: XmlNode) -> Self { Self(node.id()) }

    pub(crate) fn is_same_node(&self, other: &Self) -> bool { self.0 == other.0 }

    fn resolve(&self, transaction: &YrsTransaction) -> Option<XmlNode> {
        let tx = transaction.transaction();
        let tx = tx.as_ref()?;
        self.0.get_branch(tx).filter(|branch| !branch.is_deleted())
            .and_then(|branch| XmlNode::try_from(branch).ok())
    }

    pub(crate) fn raw_ptr(&self, transaction: &YrsTransaction) -> Result<YrsCollectionPtr, CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        Ok(YrsCollectionPtr::from(node.as_ref()))
    }

    pub(crate) fn kind(&self, transaction: &YrsTransaction) -> String {
        match self.resolve(transaction) {
            Some(XmlNode::Fragment(_)) => "fragment", Some(XmlNode::Element(_)) => "element",
            Some(XmlNode::Text(_)) => "text", None => "missing"
        }.into()
    }

    pub(crate) fn tag(&self, transaction: &YrsTransaction) -> Option<String> {
        match self.resolve(transaction) { Some(XmlNode::Element(value)) => Some(value.tag().to_string()), _ => None }
    }

    pub(crate) fn length(&self, transaction: &YrsTransaction) -> u32 {
        let Some(node) = self.resolve(transaction) else { return 0 };
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        match node {
            XmlNode::Fragment(value) => value.len(tx),
            XmlNode::Element(value) => value.len(tx),
            XmlNode::Text(value) => value.len(tx),
        }
    }

    pub(crate) fn child(&self, transaction: &YrsTransaction, index: u32) -> Option<Arc<Self>> {
        let node = self.resolve(transaction)?;
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        match node {
            XmlNode::Fragment(value) => value.get(tx, index),
            XmlNode::Element(value) => value.get(tx, index),
            XmlNode::Text(_) => None,
        }.map(|child| Arc::new(Self(child.id())))
    }

    pub(crate) fn insert_element(&self, transaction: &YrsTransaction, index: u32, tag: String) -> Result<Arc<Self>, CodingError> {
        if index > self.length(transaction) { return Err(CodingError::InvalidOperation) }
        let parent = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let prelim = XmlElementPrelim::empty(tag);
        let node = match parent {
            XmlNode::Fragment(value) => value.insert(tx, index, prelim),
            XmlNode::Element(value) => value.insert(tx, index, prelim),
            XmlNode::Text(_) => return Err(CodingError::InvalidOperation),
        };
        Ok(Arc::new(Self(XmlNode::Element(node).id())))
    }

    pub(crate) fn insert_text(&self, transaction: &YrsTransaction, index: u32) -> Result<Arc<Self>, CodingError> {
        if index > self.length(transaction) { return Err(CodingError::InvalidOperation) }
        let parent = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let prelim = XmlTextPrelim::new("");
        let node = match parent {
            XmlNode::Fragment(value) => value.insert(tx, index, prelim),
            XmlNode::Element(value) => value.insert(tx, index, prelim),
            XmlNode::Text(_) => return Err(CodingError::InvalidOperation),
        };
        Ok(Arc::new(Self(XmlNode::Text(node).id())))
    }

    pub(crate) fn remove_children(&self, transaction: &YrsTransaction, index: u32, length: u32) -> Result<(), CodingError> {
        if index.checked_add(length).map_or(true, |end| end > self.length(transaction)) { return Err(CodingError::InvalidOperation) }
        let parent = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        match parent {
            XmlNode::Fragment(value) => value.remove_range(tx, index, length),
            XmlNode::Element(value) => value.remove_range(tx, index, length),
            XmlNode::Text(_) => return Err(CodingError::InvalidOperation),
        };
        Ok(())
    }

    pub(crate) fn attributes(&self, transaction: &YrsTransaction) -> Result<Vec<YrsXmlAttribute>, CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        if matches!(node, XmlNode::Fragment(_)) { return Ok(Vec::new()) }
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let map = MapRef::from(BranchPtr::from(node.as_ref()));
        map.iter(tx).map(|(key, value)| {
            let Value::Any(any) = value else { return Err(CodingError::InvalidOperation) };
            let mut json = String::new();
            any.to_json(&mut json);
            Ok(YrsXmlAttribute { key: key.to_string(), value_json: json })
        }).collect()
    }

    pub(crate) fn set_attribute(&self, transaction: &YrsTransaction, key: String, value_json: String) -> Result<(), CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        if matches!(node, XmlNode::Fragment(_)) { return Err(CodingError::InvalidOperation) }
        let value = Any::from_json(&value_json).map_err(|_| CodingError::DecodingError)?;
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let map = MapRef::from(BranchPtr::from(node.as_ref()));
        map.insert(tx, key, value);
        Ok(())
    }

    pub(crate) fn remove_attribute(&self, transaction: &YrsTransaction, key: String) -> Result<(), CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        if matches!(node, XmlNode::Fragment(_)) { return Err(CodingError::InvalidOperation) }
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let map = MapRef::from(BranchPtr::from(node.as_ref()));
        map.remove(tx, &key);
        Ok(())
    }

    pub(crate) fn text(&self, transaction: &YrsTransaction) -> Result<String, CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        match node { XmlNode::Text(node) => Ok(node.get_string(tx)), _ => Err(CodingError::InvalidOperation) }
    }

    pub(crate) fn delta(&self, transaction: &YrsTransaction) -> Result<Vec<YrsXmlTextRun>, CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let XmlNode::Text(node) = node else { return Err(CodingError::InvalidOperation) };
        node.diff(tx, YChange::identity).into_iter().map(|part| {
            let Value::Any(Any::String(text)) = part.insert else { return Err(CodingError::InvalidOperation) };
            let attrs = YrsAttrs::from(*part.attributes.unwrap_or_default());
            Ok(YrsXmlTextRun { text: text.to_string(), attributes_json: attrs.into() })
        }).collect()
    }

    pub(crate) fn insert_string(&self, transaction: &YrsTransaction, index: u32, value: String, attributes_json: String) -> Result<(), CodingError> {
        if index > self.length(transaction) { return Err(CodingError::InvalidOperation) }
        let XmlNode::Text(node) = self.resolve(transaction).ok_or(CodingError::InvalidOperation)? else { return Err(CodingError::InvalidOperation) };
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        node.insert_with_attributes(tx, index, &value, YrsAttrs::from(attributes_json).0);
        Ok(())
    }

    pub(crate) fn format(&self, transaction: &YrsTransaction, index: u32, length: u32, attributes_json: String) -> Result<(), CodingError> {
        if index.checked_add(length).map_or(true, |end| end > self.length(transaction)) { return Err(CodingError::InvalidOperation) }
        let XmlNode::Text(node) = self.resolve(transaction).ok_or(CodingError::InvalidOperation)? else { return Err(CodingError::InvalidOperation) };
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        node.format(tx, index, length, YrsAttrs::from(attributes_json).0);
        Ok(())
    }

    pub(crate) fn remove_text(&self, transaction: &YrsTransaction, index: u32, length: u32) -> Result<(), CodingError> {
        if index.checked_add(length).map_or(true, |end| end > self.length(transaction)) { return Err(CodingError::InvalidOperation) }
        let XmlNode::Text(node) = self.resolve(transaction).ok_or(CodingError::InvalidOperation)? else { return Err(CodingError::InvalidOperation) };
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        node.remove_range(tx, index, length);
        Ok(())
    }

    pub(crate) fn relative_position(&self, transaction: &YrsTransaction, index: u32, association: i32) -> Result<Vec<u8>, CodingError> {
        let node = self.resolve(transaction).ok_or(CodingError::InvalidOperation)?;
        let length = self.length(transaction);
        if index > length { return Err(CodingError::InvalidOperation) }
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let assoc = if association < 0 { Assoc::Before } else { Assoc::After };
        StickyIndex::at(tx, BranchPtr::from(node.as_ref()), index, assoc)
            .or_else(|| (index == length && assoc == Assoc::After)
                .then(|| StickyIndex::from_type(tx, &node, assoc)))
            .map(|position| {
                let mut encoded = position.encode_v1();
                encoded.pop(); // Yrs writes one byte for its normalized association.
                encoded.extend(association_bytes(association));
                encoded
            })
            .ok_or(CodingError::InvalidOperation)
    }

    pub(crate) fn resolve_relative_position(&self, transaction: &YrsTransaction, encoded: Vec<u8>) -> Result<Option<u32>, CodingError> {
        let (position, _) = decode_relative_position(&encoded)?;
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        Ok(position.get_offset(tx)
            .filter(|offset| offset.branch.id() == self.0 && !offset.branch.is_deleted())
            .map(|offset| offset.index))
    }
}

#[cfg(test)]
mod tests {
    use crate::doc::YrsDoc;

    #[test]
    fn nested_handle_survives_transactions_and_rejects_deletion() {
        let doc = YrsDoc::new();
        let root = doc.get_xml_fragment("default".into());
        let tx = doc.transact(None);
        let paragraph = root.insert_element(&tx, 0, "paragraph".into()).unwrap();
        let text = paragraph.insert_text(&tx, 0).unwrap();
        text.insert_string(&tx, 0, "A🌍".into(), "{}".into()).unwrap();
        tx.free();

        let tx = doc.transact(None);
        assert_eq!(text.text(&tx).unwrap(), "A🌍");
        assert_eq!(text.length(&tx), 3); // UTF-16 code units
        let encoded = tx.transaction_encode_state_as_update();
        tx.free();

        let remote = YrsDoc::new();
        let remote_root = remote.get_xml_fragment("default".into());
        let remote_tx = remote.transact(None);
        remote_tx.transaction_apply_update(encoded).unwrap();
        let remote_text = remote_root.child(&remote_tx, 0).unwrap().child(&remote_tx, 0).unwrap();
        assert_eq!(remote_text.text(&remote_tx).unwrap(), "A🌍");
        remote_tx.free();

        let tx = doc.transact(None);
        root.remove_children(&tx, 0, 1).unwrap();
        tx.free();
        let tx = doc.transact(None);
        assert_eq!(paragraph.kind(&tx), "missing");
        assert!(text.text(&tx).is_err());
        tx.free();
    }

    #[test]
    fn upstream_yrs_applies_delete_before_insert() {
        use base64::Engine;
        use yrs::updates::decoder::Decode;
        use yrs::{Doc, GetString, Transact, Update, XmlFragment, XmlOut as XmlNode};

        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../Tests/YSwiftTests/Fixtures/convergence-v1.json")).unwrap();
        let case = fixture["cases"].as_array().unwrap().iter()
            .find(|item| item["id"] == "unicode-delete-only").unwrap();
        let updates = case["updates"].as_array().unwrap();
        let doc = Doc::new();
        let root = doc.get_or_insert_xml_fragment("default");
        for index in [1, 0, 2, 0] {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(updates[index].as_str().unwrap()).unwrap();
            let update = Update::decode_v1(&bytes).unwrap();
            doc.transact_mut().apply_update(update).unwrap();
        }
        let tx = doc.transact();
        let XmlNode::Element(paragraph) = root.get(&tx, 0).unwrap() else { panic!("missing paragraph") };
        let XmlNode::Text(text) = paragraph.get(&tx, 0).unwrap() else { panic!("missing text") };
        assert_eq!(text.get_string(&tx), "Aé🇺🇳Z 日本");
    }
}
