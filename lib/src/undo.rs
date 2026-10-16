use crate::doc::{YrsCollectionPtr, YrsOrigin};
use crate::subscription::subscription_key;
use crate::subscription::YSubscription;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard};
use yrs::undo::EventKind;
use yrs::{Doc, ReadTxn, StateVector, Transact};

pub(crate) struct YrsUndoManager {
    manager: Arc<Mutex<yrs::undo::UndoManager<u64>>>,
    doc: Doc,
    scopes: Mutex<BTreeSet<String>>,
    origins: Mutex<BTreeSet<Vec<u8>>>,
}

unsafe impl Send for YrsUndoManager {}
unsafe impl Sync for YrsUndoManager {}

impl YrsUndoManager {
    pub(crate) fn new(manager: yrs::undo::UndoManager<u64>, doc: Doc) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            doc,
            scopes: Mutex::new(BTreeSet::new()),
            origins: Mutex::new(BTreeSet::new()),
        }
    }

    #[inline]
    fn acquire_lock(&self) -> MutexGuard<'_, yrs::undo::UndoManager<u64>> {
        // unwrap should be safe, as the only occasion to cause error would be a panic
        // while holding a lock and all operations holding a lock here only do so for
        // a time needed to perform a non-panicing operation
        self.manager.lock().unwrap()
    }

    /// Read-only actual maintained stack metadata, oldest item first. Pointer
    /// ownership remains with the stack; Swift must take unretained references.
    pub(crate) fn preserve_changed_xml(&self, enabled: bool) {
        self.acquire_lock().preserve_changed_xml(enabled);
    }

    pub(crate) fn undo_metadata(&self) -> Vec<u64> {
        self.acquire_lock()
            .undo_stack()
            .iter()
            .map(|item| item.meta)
            .collect()
    }
    pub(crate) fn redo_metadata(&self) -> Vec<u64> {
        self.acquire_lock()
            .redo_stack()
            .iter()
            .map(|item| item.meta)
            .collect()
    }

    /// Retire complete current items in the same maintained owner. Metadata
    /// pointers are returned only after success so Swift releases exactly those
    /// stack-owned references. Failed prevalidation changes no pointer lifetime.
    pub(crate) fn retire_history_items(
        &self,
        undo_indices: Vec<u64>,
        redo_indices: Vec<u64>,
    ) -> Result<Vec<u64>, YrsUndoError> {
        let mut manager = self.acquire_lock();
        let decode = |indices: Vec<u64>| -> Result<Vec<usize>, YrsUndoError> {
            indices
                .into_iter()
                .map(|index| usize::try_from(index).map_err(|_| YrsUndoError::InvalidHistory))
                .collect()
        };
        let undo = decode(undo_indices)?;
        let redo = decode(redo_indices)?;
        let mut metadata = Vec::new();
        for (indices, stack) in [(&undo, manager.undo_stack()), (&redo, manager.redo_stack())] {
            for &index in indices {
                metadata.push(stack.get(index).ok_or(YrsUndoError::InvalidHistory)?.meta);
            }
        }
        manager
            .retire_history_items(&undo, &redo)
            .map_err(|_| YrsUndoError::InvalidHistory)?;
        Ok(metadata)
    }

    pub(crate) fn add_origin(&self, origin: YrsOrigin) {
        let mut m = self.acquire_lock();
        let encoded: yrs::Origin = origin.clone().into();
        self.origins
            .lock()
            .unwrap()
            .insert(encoded.as_ref().to_vec());
        m.include_origin(origin)
    }

    pub(crate) fn remove_origin(&self, origin: YrsOrigin) {
        let mut m = self.acquire_lock();
        let encoded: yrs::Origin = origin.clone().into();
        self.origins.lock().unwrap().remove(encoded.as_ref());
        m.exclude_origin(origin);
    }

    pub(crate) fn add_scope(&self, tracked_ref: YrsCollectionPtr) {
        let mut m = self.acquire_lock();
        m.expand_scope(&self.doc, &tracked_ref);
        self.scopes
            .lock()
            .unwrap()
            .insert(serde_json::to_string(&tracked_ref.as_ref().id()).unwrap());
    }

    /// Snapshot maintained Yrs stacks and the exact retained CRDT in one read.
    /// Metadata pointers are process-local and deliberately kept outside JSON.
    pub(crate) fn export_history(
        &self,
        scope_token: String,
    ) -> Result<YrsUndoSnapshot, YrsUndoError> {
        if !self.doc.skip_gc() || scope_token.is_empty() {
            return Err(YrsUndoError::InvalidHistory);
        }
        let manager = self.acquire_lock();
        let txn = self
            .doc
            .try_transact()
            .map_err(|_| YrsUndoError::PendingTransaction)?;
        let mut metadata = Vec::new();
        let mut encode =
            |items: &[yrs::undo::StackItem<u64>]| -> Result<serde_json::Value, YrsUndoError> {
                let mut values = Vec::new();
                for item in items {
                    let mut value =
                        serde_json::to_value(item).map_err(|_| YrsUndoError::InvalidHistory)?;
                    value["meta"] = serde_json::json!(metadata.len());
                    metadata.push(item.meta);
                    values.push(value);
                }
                Ok(serde_json::Value::Array(values))
            };
        let undo = encode(manager.undo_stack())?;
        let redo = encode(manager.redo_stack())?;
        let json = serde_json::json!({
            "document_guid": self.doc.guid().as_ref(), "scope_token": scope_token,
            "document_state": txn.encode_diff_v1(&StateVector::default()),
            "scopes": *self.scopes.lock().unwrap(), "origins": *self.origins.lock().unwrap(),
            "undo_stack": undo, "redo_stack": redo
        })
        .to_string();
        Ok(YrsUndoSnapshot { json, metadata })
    }

    pub(crate) fn validate_history(
        doc: &Doc,
        scopes: &[YrsCollectionPtr],
        json: &str,
        scope_token: &str,
    ) -> Result<serde_json::Value, YrsUndoError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|_| YrsUndoError::InvalidHistory)?;
        let txn = doc
            .try_transact()
            .map_err(|_| YrsUndoError::PendingTransaction)?;
        for scope in scopes {
            let branch = scope
                .as_ref()
                .id()
                .get_branch(&txn)
                .ok_or(YrsUndoError::InvalidHistory)?;
            if !std::ptr::eq(branch.as_ref(), scope.as_ref()) {
                return Err(YrsUndoError::InvalidHistory);
            }
        }
        let scope_ids: BTreeSet<String> = scopes
            .iter()
            .map(|scope| serde_json::to_string(&scope.as_ref().id()).unwrap())
            .collect();
        if !doc.skip_gc()
            || scope_token.is_empty()
            || value["document_guid"].as_str() != Some(doc.guid().as_ref())
            || value["scope_token"].as_str() != Some(scope_token)
            || value["document_state"]
                != serde_json::json!(txn.encode_diff_v1(&StateVector::default()))
            || value["scopes"] != serde_json::json!(scope_ids)
        {
            return Err(YrsUndoError::InvalidHistory);
        }
        let expected_keys: BTreeSet<&str> = [
            "document_guid",
            "scope_token",
            "document_state",
            "scopes",
            "origins",
            "undo_stack",
            "redo_stack",
        ]
        .into_iter()
        .collect();
        if value
            .as_object()
            .ok_or(YrsUndoError::InvalidHistory)?
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != expected_keys
        {
            return Err(YrsUndoError::InvalidHistory);
        }
        let mut ordinal = 0u64;
        for key in ["undo_stack", "redo_stack"] {
            let items = value[key].as_array().ok_or(YrsUndoError::InvalidHistory)?;
            for item in items {
                if item["meta"].as_u64() != Some(ordinal) {
                    return Err(YrsUndoError::InvalidHistory);
                }
                ordinal += 1;
                if item["doc"] != value["document_guid"] {
                    return Err(YrsUndoError::InvalidHistory);
                }
                serde_json::from_value::<yrs::undo::StackItem<u64>>(item.clone())
                    .map_err(|_| YrsUndoError::InvalidHistory)?;
            }
        }
        serde_json::from_value::<Vec<Vec<u8>>>(value["origins"].clone())
            .map_err(|_| YrsUndoError::InvalidHistory)?;
        Ok(value)
    }

    pub(crate) fn restore(
        doc: Doc,
        scopes: Vec<YrsCollectionPtr>,
        json: String,
        scope_token: String,
        metadata: Vec<u64>,
    ) -> Result<Self, YrsUndoError> {
        let value = Self::validate_history(&doc, &scopes, &json, &scope_token)?;
        if metadata.len()
            != value["undo_stack"].as_array().unwrap().len()
                + value["redo_stack"].as_array().unwrap().len()
        {
            return Err(YrsUndoError::InvalidHistory);
        }
        let decode = |key: &str| -> Result<Vec<yrs::undo::StackItem<u64>>, YrsUndoError> {
            let mut items: Vec<yrs::undo::StackItem<u64>> =
                serde_json::from_value(value[key].clone())
                    .map_err(|_| YrsUndoError::InvalidHistory)?;
            for item in &mut items {
                item.meta = *metadata
                    .get(item.meta as usize)
                    .ok_or(YrsUndoError::InvalidHistory)?;
            }
            Ok(items)
        };
        let mut options = yrs::undo::Options::default();
        options.init_undo_stack = decode("undo_stack")?;
        options.init_redo_stack = decode("redo_stack")?;
        let origins: Vec<Vec<u8>> = serde_json::from_value(value["origins"].clone())
            .map_err(|_| YrsUndoError::InvalidHistory)?;
        options.tracked_origins = origins
            .iter()
            .map(|origin| yrs::Origin::from(origin.as_slice()))
            .collect();
        let manager = Self::new(yrs::undo::UndoManager::with_options(options), doc);
        *manager.origins.lock().unwrap() = origins.into_iter().collect();
        for scope in scopes {
            manager.add_scope(scope);
        }
        Ok(manager)
    }

    pub(crate) fn undo(&self) -> Result<bool, YrsUndoError> {
        let mut m = self.acquire_lock();
        Ok(m.undo_blocking())
    }

    pub(crate) fn redo(&self) -> Result<bool, YrsUndoError> {
        let mut m = self.acquire_lock();
        Ok(m.redo_blocking())
    }

    pub(crate) fn clear(&self) -> Result<(), YrsUndoError> {
        let mut m = self.acquire_lock();
        m.clear_all();
        Ok(())
    }

    pub(crate) fn wrap_changes(&self) {
        let mut m = self.acquire_lock();
        m.reset();
    }

    pub(crate) fn observe_added(
        &self,
        delegate: Box<dyn YrsUndoManagerObservationDelegate>,
    ) -> Arc<YSubscription> {
        let key = subscription_key();
        let manager = self.manager.clone();
        self.acquire_lock()
            .observe_item_added(key.clone(), move |_, e| {
                *e.meta_mut() = delegate.call(YrsUndoEvent::new(e), *e.meta_mut());
            });
        Arc::new(YSubscription::new(move || {
            manager.lock().unwrap().unobserve_item_added(key);
        }))
    }

    pub(crate) fn observe_updated(
        &self,
        delegate: Box<dyn YrsUndoManagerObservationDelegate>,
    ) -> Arc<YSubscription> {
        let key = subscription_key();
        let manager = self.manager.clone();
        self.acquire_lock()
            .observe_item_updated(key.clone(), move |_, e| {
                *e.meta_mut() = delegate.call(YrsUndoEvent::new(e), *e.meta_mut());
            });
        Arc::new(YSubscription::new(move || {
            manager.lock().unwrap().unobserve_item_updated(key);
        }))
    }

    pub(crate) fn observe_popped(
        &self,
        delegate: Box<dyn YrsUndoManagerObservationDelegate>,
    ) -> Arc<YSubscription> {
        let key = subscription_key();
        let manager = self.manager.clone();
        self.acquire_lock()
            .observe_item_popped(key.clone(), move |_, e| {
                *e.meta_mut() = delegate.call(YrsUndoEvent::new(e), *e.meta_mut());
            });
        Arc::new(YSubscription::new(move || {
            manager.lock().unwrap().unobserve_item_popped(key);
        }))
    }
}

pub(crate) struct YrsUndoSnapshot {
    pub(crate) json: String,
    pub(crate) metadata: Vec<u64>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum YrsUndoError {
    #[error("Operations failed - there's already an active transaction on a current document")]
    PendingTransaction,
    #[error("Undo history does not match the retained document, owner or tracked scope")]
    InvalidHistory,
}

pub(crate) trait YrsUndoManagerObservationDelegate: Send + Sync + Debug {
    fn call(&self, e: Arc<YrsUndoEvent>, ptr: u64) -> u64;
}

pub(crate) struct YrsUndoEvent {
    inner: &'static mut yrs::undo::Event<u64>,
}

unsafe impl Send for YrsUndoEvent {}
unsafe impl Sync for YrsUndoEvent {}

impl YrsUndoEvent {
    fn new(inner: &mut yrs::undo::Event<u64>) -> Arc<Self> {
        let inner = unsafe { std::mem::transmute(inner) };
        Arc::new(YrsUndoEvent { inner })
    }

    pub(crate) fn origin(&self) -> Option<YrsOrigin> {
        self.inner.origin().cloned().map(YrsOrigin::from)
    }
    pub(crate) fn kind(&self) -> YrsUndoEventKind {
        match self.inner.kind() {
            EventKind::Undo => YrsUndoEventKind::Undo,
            EventKind::Redo => YrsUndoEventKind::Redo,
        }
    }
    pub(crate) fn has_changed(&self, shared_ref: YrsCollectionPtr) -> bool {
        self.inner.has_changed(&shared_ref)
    }
}

#[repr(u8)]
pub(crate) enum YrsUndoEventKind {
    Undo,
    Redo,
}

#[cfg(test)]
mod durable_tests {
    use super::*;
    use yrs::updates::decoder::Decode;
    use yrs::{GetString, Options, ReadTxn, StateVector, Text, Transact, Update};

    // A removed XML control can be restored, then its preceding attribute edit
    // undone after reopening the exact retained owner. Rendering alone is not
    // sufficient: the snapshot below independently checks clocks and deletions.
    #[test]
    fn restored_xml_attribute_history_roundtrips_exact_retained_state() {
        use yrs::{Xml, XmlElementPrelim, XmlFragment};
        let doc = Doc::with_options(Options {
            guid: "xml-history-owner".into(), skip_gc: true, ..Options::default()
        });
        let root = doc.get_or_insert_xml_fragment("default");
        let manager = YrsUndoManager::new(yrs::undo::UndoManager::new(), doc.clone());
        manager.add_scope(YrsCollectionPtr::from(root.as_ref()));
        manager.add_origin(YrsOrigin::from(yrs::Origin::from("local")));
        manager.preserve_changed_xml(true);
        let anchor = {
            let mut txn = doc.transact_mut_with("local");
            let anchor = root.insert(&mut txn, 0, XmlElementPrelim::empty("relayAnchor"));
            anchor.insert_attribute(&mut txn, "name", "later");
            anchor
        };
        manager.wrap_changes();
        anchor.insert_attribute(&mut doc.transact_mut_with("local"), "name", "later_more");
        manager.wrap_changes();
        root.remove_range(&mut doc.transact_mut_with("local"), 0, 1);
        manager.wrap_changes();
        assert!(manager.undo().unwrap());
        let archive = manager.export_history("account/paper/owner".into()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&archive.json).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(value["document_state"].clone()).unwrap();
        let reopened = Doc::with_options(Options {
            guid: "xml-history-owner".into(), skip_gc: true, ..Options::default()
        });
        let reopened_root = reopened.get_or_insert_xml_fragment("default");
        reopened.transact_mut().apply_update(Update::decode_v1(&bytes).unwrap()).unwrap();
        assert_eq!(root.get_string(&doc.transact()), reopened_root.get_string(&reopened.transact()));
        assert_eq!(doc.transact().snapshot(), reopened.transact().snapshot());
        // Wire-only import must still refuse: it lacks private redone links.
        assert!(YrsUndoManager::restore(reopened.clone(),
            vec![YrsCollectionPtr::from(reopened_root.as_ref())], archive.json.clone(),
            "account/paper/owner".into(), archive.metadata.clone()).is_err());
        let preview = yrs::retained_history::fork(&doc, 100).unwrap();
        let preview_root = preview.get_or_insert_xml_fragment("default");
        assert!(YrsUndoManager::restore(preview.clone(),
            vec![YrsCollectionPtr::from(root.as_ref())], archive.json.clone(),
            "account/paper/owner".into(), archive.metadata.clone()).is_err());
        assert!(YrsUndoManager::restore(preview.clone(),
            vec![YrsCollectionPtr::from(preview_root.as_ref())], archive.json.clone(),
            "wrong-scope".into(), archive.metadata.clone()).is_err());
        let restored = YrsUndoManager::restore(preview.clone(),
            vec![YrsCollectionPtr::from(preview_root.as_ref())], archive.json,
            "account/paper/owner".into(), archive.metadata).unwrap();
        assert!(restored.undo().unwrap());
        assert!(preview_root.get_string(&preview.transact()).contains("name=\"later\""));
        assert!(!preview_root.get_string(&preview.transact()).contains("later_more"));
        assert!(restored.redo().unwrap());
        assert_eq!(preview_root.get_string(&preview.transact()), root.get_string(&doc.transact()));
        assert_eq!(doc.transact().encode_diff_v1(&StateVector::default()), bytes);
    }

    // Reopening a deleted selection must retain original CRDT item IDs and redo,
    // while an untracked peer edit must survive a later local undo.
    #[test]
    fn reopen_deleted_text_redo_and_peer_edit() {
        let doc = Doc::with_options(Options {
            guid: "paper-guid".into(),
            skip_gc: true,
            ..Options::default()
        });
        let text = doc.get_or_insert_text("default");
        text.insert(&mut doc.transact_mut(), 0, "alpha beta");
        let mut inner = yrs::undo::UndoManager::new();
        inner.expand_scope(&doc, &text);
        let manager = YrsUndoManager::new(inner, doc.clone());
        manager.add_scope(YrsCollectionPtr::from(text.as_ref()));
        manager.add_origin(YrsOrigin::from(yrs::Origin::from("local")));
        text.remove_range(&mut doc.transact_mut_with("local"), 6, 4);
        let snapshot = manager
            .export_history("account/paper/owner".into())
            .unwrap();
        let reopened = Doc::with_options(Options {
            guid: "paper-guid".into(),
            skip_gc: true,
            ..Options::default()
        });
        let reopened_text = reopened.get_or_insert_text("default");
        let update: Vec<u8> = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(&snapshot.json).unwrap()["document_state"]
                .clone(),
        )
        .unwrap();
        reopened
            .transact_mut()
            .apply_update(Update::decode_v1(&update).unwrap())
            .unwrap();
        let scope = vec![YrsCollectionPtr::from(reopened_text.as_ref())];
        let restored = YrsUndoManager::restore(
            reopened.clone(),
            scope.clone(),
            snapshot.json.clone(),
            "account/paper/owner".into(),
            snapshot.metadata.clone(),
        )
        .unwrap();
        assert!(restored.undo().unwrap());
        assert_eq!(reopened_text.get_string(&reopened.transact()), "alpha beta");
        // The redo stack itself must survive another process/document lifetime.
        let undo_snapshot = restored
            .export_history("account/paper/owner".into())
            .unwrap();
        let second = Doc::with_options(Options {
            guid: "paper-guid".into(),
            skip_gc: true,
            ..Options::default()
        });
        let second_text = second.get_or_insert_text("default");
        let second_json: serde_json::Value = serde_json::from_str(&undo_snapshot.json).unwrap();
        let second_update: Vec<u8> =
            serde_json::from_value(second_json["document_state"].clone()).unwrap();
        second
            .transact_mut()
            .apply_update(Update::decode_v1(&second_update).unwrap())
            .unwrap();
        let second_scope = vec![YrsCollectionPtr::from(second_text.as_ref())];
        let second_manager = YrsUndoManager::restore(
            second.clone(),
            second_scope,
            undo_snapshot.json,
            "account/paper/owner".into(),
            undo_snapshot.metadata,
        )
        .unwrap();
        assert!(second_manager.redo().unwrap());
        assert_eq!(second_text.get_string(&second.transact()), "alpha ");
        assert!(restored.redo().unwrap());
        assert_eq!(reopened_text.get_string(&reopened.transact()), "alpha ");
        reopened_text.insert(&mut reopened.transact_mut_with("remote"), 6, "peer");
        assert!(restored.undo().unwrap());
        assert_eq!(
            reopened_text.get_string(&reopened.transact()),
            "alpha betapeer"
        );
        let before = reopened.transact().encode_diff_v1(&StateVector::default());
        assert!(YrsUndoManager::restore(
            reopened.clone(),
            scope,
            snapshot.json,
            "wrong-owner".into(),
            snapshot.metadata
        )
        .is_err());
        assert_eq!(
            before,
            reopened.transact().encode_diff_v1(&StateVector::default())
        );
    }

    #[test]
    fn history_admission_requires_exact_owner_document_state_and_scope() {
        let doc = Doc::with_options(Options {
            guid: "original".into(),
            skip_gc: true,
            ..Options::default()
        });
        let text = doc.get_or_insert_text("default");
        let manager = YrsUndoManager::new(yrs::undo::UndoManager::new(), doc.clone());
        manager.add_scope(YrsCollectionPtr::from(text.as_ref()));
        text.insert(&mut doc.transact_mut(), 0, "retained");
        let archive = manager.export_history("owner".into()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&archive.json).unwrap();
        let state: Vec<u8> = serde_json::from_value(value["document_state"].clone()).unwrap();
        for (guid, retain) in [("wrong", true), ("original", false)] {
            let target = Doc::with_options(Options {
                guid: guid.into(),
                skip_gc: retain,
                ..Options::default()
            });
            let scope = target.get_or_insert_text("default");
            target
                .transact_mut()
                .apply_update(Update::decode_v1(&state).unwrap())
                .unwrap();
            assert!(YrsUndoManager::restore(
                target,
                vec![YrsCollectionPtr::from(scope.as_ref())],
                archive.json.clone(),
                "owner".into(),
                archive.metadata.clone()
            )
            .is_err());
        }
        let other_replica = Doc::with_options(Options {
            guid: "original".into(),
            skip_gc: true,
            ..Options::default()
        });
        let other_text = other_replica.get_or_insert_text("default");
        assert!(YrsUndoManager::restore(
            doc.clone(),
            vec![YrsCollectionPtr::from(other_text.as_ref())],
            archive.json.clone(),
            "owner".into(),
            archive.metadata.clone()
        )
        .is_err());
        let wrong_scope = doc.get_or_insert_text("elsewhere");
        assert!(YrsUndoManager::restore(
            doc.clone(),
            vec![YrsCollectionPtr::from(wrong_scope.as_ref())],
            archive.json.clone(),
            "owner".into(),
            archive.metadata.clone()
        )
        .is_err());
        text.insert(&mut doc.transact_mut_with("peer"), 0, "new ");
        let before = doc.transact().encode_diff_v1(&StateVector::default());
        assert!(YrsUndoManager::restore(
            doc.clone(),
            vec![YrsCollectionPtr::from(text.as_ref())],
            archive.json,
            "owner".into(),
            archive.metadata
        )
        .is_err());
        assert_eq!(
            before,
            doc.transact().encode_diff_v1(&StateVector::default())
        );
    }
}
