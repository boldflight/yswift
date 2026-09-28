use crate::error::CodingError;
use yrs::{updates::decoder::Decode, Update};

/// Read community deletion metadata without applying or rewriting updates.
/// Reconstructed state may already declare all cascaded deletions.
pub(crate) fn has_additional_deletions_v1(before: Vec<u8>, after: Vec<u8>, declared_by: Vec<u8>) -> Result<bool, CodingError> {
    let before = Update::decode_v1(&before).map_err(|_| CodingError::DecodingError)?;
    let after = Update::decode_v1(&after).map_err(|_| CodingError::DecodingError)?;
    let incoming = Update::decode_v1(&declared_by).map_err(|_| CodingError::DecodingError)?;
    let declared = before.delete_set().merge(incoming.delete_set());
    Ok(!after.delete_set().diff(&declared).is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use yrs::{Doc, ReadTxn, StateVector, Transact};

    fn check_fixture(json: &str, operation: &str) {
        let fixture: serde_json::Value = serde_json::from_str(json).unwrap();
        let bytes = |key: &str| STANDARD.decode(fixture[key].as_str().unwrap()).unwrap();
        let seed = bytes("seed"); let peer = bytes("peer_update"); let change = bytes(operation);
        for concurrent in [false, true] {
            let doc = Doc::new(); let mut tx = doc.transact_mut();
            tx.apply_update(Update::decode_v1(&seed).unwrap()).unwrap();
            if concurrent { tx.apply_update(Update::decode_v1(&peer).unwrap()).unwrap(); }
            let before = tx.encode_state_as_update_v1(&StateVector::default());
            tx.apply_update(Update::decode_v1(&change).unwrap()).unwrap();
            let after = tx.encode_state_as_update_v1(&StateVector::default());
            assert_eq!(has_additional_deletions_v1(before.clone(), after.clone(), change.clone()).unwrap(), concurrent);
            // Reconstructed state cannot prove the original sender's intent.
            assert!(!has_additional_deletions_v1(before, after.clone(), after).unwrap());
        }
    }

    #[test]
    fn detects_actual_yjs_structural_cascades() {
        check_fixture(include_str!("../../Tests/YSwiftTests/Fixtures/paragraph-join-v3.json"), "browser_join_update");
        check_fixture(include_str!("../../Tests/YSwiftTests/Fixtures/heading-conversion-v3.json"), "browser_conversion_update");
    }

    #[test]
    fn refuses_malformed_updates() {
        let empty = vec![0, 0];
        for position in 0..3 {
            let mut args = [empty.clone(), empty.clone(), empty.clone()]; args[position] = vec![255];
            assert!(has_additional_deletions_v1(args[0].clone(), args[1].clone(), args[2].clone()).is_err());
        }
    }
}
