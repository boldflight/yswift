use crate::error::CodingError;
use std::sync::{Arc, Mutex};
use yrs::sync::awareness::{Awareness, AwarenessUpdate, AwarenessUpdateSummary};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ClientID, Doc};

pub(crate) struct YrsAwareness(Mutex<Awareness>);

pub(crate) struct YrsAwarenessState {
    pub(crate) client_id: u64,
    pub(crate) clock: u32,
    pub(crate) last_updated_millis: u64,
    pub(crate) json: Option<String>,
}

pub(crate) struct YrsAwarenessChanges {
    pub(crate) added: Vec<u64>,
    pub(crate) updated: Vec<u64>,
    pub(crate) removed: Vec<u64>,
}

impl From<AwarenessUpdateSummary> for YrsAwarenessChanges {
    fn from(summary: AwarenessUpdateSummary) -> Self {
        Self {
            added: summary.added.into_iter().map(|id| id.get()).collect(),
            updated: summary.updated.into_iter().map(|id| id.get()).collect(),
            removed: summary.removed.into_iter().map(|id| id.get()).collect(),
        }
    }
}

fn client_id(value: u64) -> Result<ClientID, CodingError> {
    if value >= (1_u64 << 53) { return Err(CodingError::InvalidOperation) }
    Ok(ClientID::new(value))
}

impl YrsAwareness {
    pub(crate) fn new(doc: Doc) -> Self { Self(Mutex::new(Awareness::new(doc))) }

    pub(crate) fn client_id(&self) -> u64 {
        self.0.lock().unwrap().client_id().get()
    }

    pub(crate) fn set_local_state(&self, json: String) -> Result<(), CodingError> {
        // Awareness states are JSON objects; null is represented by clear_local_state.
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|_| CodingError::DecodingError)?;
        if !value.is_object() { return Err(CodingError::InvalidOperation) }
        self.0.lock().unwrap().set_local_state_raw(json);
        Ok(())
    }

    pub(crate) fn clear_local_state(&self) {
        self.0.lock().unwrap().clean_local_state();
    }

    pub(crate) fn states(&self) -> Vec<YrsAwarenessState> {
        let awareness = self.0.lock().unwrap();
        let mut states: Vec<_> = awareness.iter().map(|(id, state)| YrsAwarenessState {
            client_id: id.get(),
            clock: state.clock,
            last_updated_millis: state.last_updated,
            json: state.data.map(|json| json.to_string()),
        }).collect();
        states.sort_by_key(|state| state.client_id);
        states
    }

    pub(crate) fn encode_update(&self) -> Result<Vec<u8>, CodingError> {
        self.0.lock().unwrap().update()
            .map(|update| update.encode_v1())
            .map_err(|_| CodingError::EncodingError)
    }

    pub(crate) fn encode_update_for_clients(&self, client_ids: Vec<u64>) -> Result<Vec<u8>, CodingError> {
        let ids = client_ids.into_iter().map(client_id).collect::<Result<Vec<_>, _>>()?;
        self.0.lock().unwrap().update_with_clients(ids)
            .map(|update| update.encode_v1())
            .map_err(|_| CodingError::InvalidOperation)
    }

    pub(crate) fn apply_update(&self, encoded: Vec<u8>) -> Result<Option<YrsAwarenessChanges>, CodingError> {
        let update = AwarenessUpdate::decode_v1(&encoded).map_err(|_| CodingError::DecodingError)?;
        for entry in update.clients.values() {
            let value: serde_json::Value = serde_json::from_str(&entry.json)
                .map_err(|_| CodingError::DecodingError)?;
            if !value.is_object() && !value.is_null() { return Err(CodingError::InvalidOperation) }
        }
        self.0.lock().unwrap().apply_update_summary(update)
            .map(|summary| summary.map(Into::into))
            .map_err(|_| CodingError::InvalidOperation)
    }

    pub(crate) fn remove_remote_state(&self, raw_id: u64) -> Result<Option<YrsAwarenessChanges>, CodingError> {
        let id = client_id(raw_id)?;
        let mut awareness = self.0.lock().unwrap();
        if id == awareness.client_id() { return Err(CodingError::InvalidOperation) }
        // Match y-protocols' remote timeout/removal: a null update at the
        // existing clock, leaving the next newer update eligible to restore it.
        let mut update = awareness.update_with_clients([id])
            .map_err(|_| CodingError::InvalidOperation)?;
        if let Some(entry) = update.clients.get_mut(&id) {
            entry.json = Arc::from("null");
        }
        awareness.apply_update_summary(update)
            .map(|summary| summary.map(Into::into))
            .map_err(|_| CodingError::InvalidOperation)
    }
}

#[cfg(test)]
mod tests {
    use super::YrsAwareness;
    use base64::Engine;
    use std::collections::HashMap;
    use yrs::Doc;
    use yrs::sync::awareness::{AwarenessUpdate, AwarenessUpdateEntry};
    use yrs::updates::encoder::Encode;
    use yrs::ClientID;

    #[test]
    fn y_protocols_wire_update_and_equal_clock_removal() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../Tests/YSwiftTests/Fixtures/awareness-v1.json")).unwrap();
        let bytes = |key: &str| base64::engine::general_purpose::STANDARD
            .decode(fixture[key].as_str().unwrap()).unwrap();
        let added = bytes("added");
        let heartbeat = bytes("heartbeat");
        let equal_removal = bytes("equalClockRemoval");
        let publisher = YrsAwareness::new(Doc::with_client_id(42));
        publisher.set_local_state(r#"{"user":{"name":"Ada"},"cursor":{"anchor":1,"head":3}}"#.into()).unwrap();
        assert_eq!(publisher.encode_update_for_clients(vec![42]).unwrap(), added);

        let receiver = YrsAwareness::new(Doc::with_client_id(77));
        assert_eq!(receiver.apply_update(added.clone()).unwrap().unwrap().added, vec![42]);
        assert_eq!(receiver.encode_update_for_clients(vec![42]).unwrap(), added);
        assert_eq!(receiver.remove_remote_state(42).unwrap().unwrap().removed, vec![42]);
        assert_eq!(receiver.encode_update_for_clients(vec![42]).unwrap(), equal_removal);
        assert_eq!(receiver.apply_update(heartbeat).unwrap().unwrap().updated, vec![42]);
        assert!(receiver.states().iter().any(|state| state.client_id == 42 && state.json.is_some()));

        let invalid = AwarenessUpdate { clients: HashMap::from([(
            ClientID::new(42), AwarenessUpdateEntry { clock: 3, json: "[]".into() }
        )]) }.encode_v1();
        assert!(receiver.apply_update(invalid).is_err());
        assert!(receiver.states().iter().any(|state| state.client_id == 42 && state.clock == 2));
    }
}
