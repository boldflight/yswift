use crate::array::YrsArray;
use crate::error::CodingError;
use crate::map::YrsMap;
use crate::text::YrsText;
use std::borrow::Borrow;
use std::cell::{RefCell, RefMut};
use std::sync::Arc;
use yrs::{
    updates::decoder::Decode, updates::encoder::Encode, ReadTxn, StateVector, TransactionMut,
    Update,
};
use yrs::{Store, WriteTxn};
use crate::doc::YrsOrigin;

pub(crate) struct YrsTransaction(pub(crate) RefCell<Option<TransactionMut<'static>>>);

pub(crate) struct YrsIntegrationStatus {
    pub(crate) has_pending_structs: bool,
    pub(crate) has_pending_deletes: bool,
}

unsafe impl Send for YrsTransaction {}
unsafe impl Sync for YrsTransaction {}

impl YrsTransaction {}

impl ReadTxn for YrsTransaction {
    fn store(&self) -> &Store {
        let mut tx = self.transaction();
        let tx = tx.as_mut().unwrap();

        // Use transmute to cast the mutable reference to the `Store` to a reference with a shorter lifetime
        unsafe { std::mem::transmute::<&mut Store, &'static Store>(tx.store_mut()) }
    }
}

impl<'doc> From<TransactionMut<'doc>> for YrsTransaction {
    fn from(txn: TransactionMut<'doc>) -> Self {
        let txn: TransactionMut<'static> = unsafe { std::mem::transmute(txn) };
        YrsTransaction(RefCell::from(Some(txn)))
    }
}

impl YrsTransaction {
    pub(crate) fn transaction(&self) -> RefMut<'_, Option<TransactionMut<'static>>> {
        self.0.borrow_mut()
    }

    pub(crate) fn origin(&self) -> Option<YrsOrigin> {
        let txn = self.0.borrow();
        txn.as_ref()?.origin().cloned().map(YrsOrigin::from)
    }

    pub(crate) fn integration_status(&self) -> YrsIntegrationStatus {
        let tx = self.transaction();
        let store = tx.as_ref().unwrap().store();
        YrsIntegrationStatus {
            has_pending_structs: store.pending_update().is_some(),
            has_pending_deletes: store.pending_ds().is_some(),
        }
    }

    pub(crate) fn transaction_encode_update(&self) -> Vec<u8> {
        self.transaction().as_ref().unwrap().encode_update_v1()
    }

    pub(crate) fn transaction_encode_state_as_update_from_sv(
        &self,
        state_vector: Vec<u8>,
    ) -> Result<Vec<u8>, CodingError> {
        let mut tx = self.transaction();
        let tx = tx.as_mut().unwrap();

        StateVector::decode_v1(state_vector.borrow())
            .map_err(|_e| CodingError::DecodingError)
            .map(|sv: StateVector| tx.encode_state_as_update_v1(&sv))
    }

    pub(crate) fn transaction_encode_state_as_update(&self) -> Vec<u8> {
        let mut tx = self.transaction();
        let tx = tx.as_mut().unwrap();
        tx.encode_state_as_update_v1(&StateVector::default())
    }

    pub(crate) fn transaction_state_vector(&self) -> Vec<u8> {
        self.transaction()
            .as_ref()
            .unwrap()
            .state_vector()
            .encode_v1()
    }

    pub(crate) fn transaction_apply_update(&self, update: Vec<u8>) -> Result<(), CodingError> {
        let decoded = Update::decode_v1(update.as_slice()).map_err(|_| CodingError::DecodingError)?;
        self.transaction().as_mut().unwrap().apply_update(decoded)
            .map_err(|_| CodingError::DecodingError)
    }

    pub(crate) fn transaction_get_text(&self, name: String) -> Option<Arc<YrsText>> {
        let tx = self.transaction();
        let tx = tx.as_ref().unwrap();
        let doc = tx.doc().clone();
        tx.get_text(name.as_str()).map(|reference| Arc::new(YrsText::new(reference, doc)))
    }

    pub(crate) fn transaction_get_array(&self, name: String) -> Option<Arc<YrsArray>> {
        let tx = self.transaction();
        let tx = tx.as_ref().unwrap();
        let doc = tx.doc().clone();
        tx.get_array(name.as_str()).map(|reference| Arc::new(YrsArray::new(reference, doc)))
    }

    pub(crate) fn transaction_get_map(&self, name: String) -> Option<Arc<YrsMap>> {
        let tx = self.transaction();
        let tx = tx.as_ref().unwrap();
        let doc = tx.doc().clone();
        tx.get_map(name.as_str()).map(|reference| Arc::new(YrsMap::new(reference, doc)))
    }

    pub(crate) fn free(&self) {
        self.0.replace(None);
    }
}

#[cfg(test)]
mod tests {
    use crate::doc::YrsDoc;
    use base64::Engine;

    #[test]
    fn integration_status_tracks_pending_yjs_structs_and_deletes() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../Tests/YSwiftTests/Fixtures/convergence-v1.json")).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        let bytes = |case_id: &str, index: usize| {
            let case = cases.iter().find(|case| case["id"] == case_id).unwrap();
            base64::engine::general_purpose::STANDARD
                .decode(case["updates"][index].as_str().unwrap()).unwrap()
        };

        let delete_doc = YrsDoc::new();
        let tx = delete_doc.transact(None);
        assert!(!tx.integration_status().has_pending_deletes);
        tx.transaction_apply_update(bytes("unicode-delete-only", 1)).unwrap();
        assert!(tx.integration_status().has_pending_deletes);
        assert!(!tx.integration_status().has_pending_structs);
        tx.free();
        let tx = delete_doc.transact(None);
        assert!(tx.integration_status().has_pending_deletes);
        tx.transaction_apply_update(bytes("unicode-delete-only", 0)).unwrap();
        assert!(!tx.integration_status().has_pending_deletes);
        tx.free();

        let struct_doc = YrsDoc::new();
        let tx = struct_doc.transact(None);
        tx.transaction_apply_update(bytes("concurrent-mark-entity-reference", 1)).unwrap();
        assert!(tx.integration_status().has_pending_structs);
        tx.free();
        let tx = struct_doc.transact(None);
        tx.transaction_apply_update(bytes("concurrent-mark-entity-reference", 0)).unwrap();
        assert!(!tx.integration_status().has_pending_structs);
        assert!(!tx.integration_status().has_pending_deletes);
        tx.free();
    }
}
