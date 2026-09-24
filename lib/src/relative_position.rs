use crate::error::CodingError;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::StickyIndex;

// Yjs stores a signed association as the final varint of a relative position.
// Yrs models only its sign (Before/After), so preserve the original wire value.
pub(crate) fn association_bytes(value: i32) -> Vec<u8> {
    let mut magnitude = value.unsigned_abs();
    let mut first = (magnitude & 0x3f) as u8;
    if value < 0 { first |= 0x40; }
    magnitude >>= 6;
    if magnitude != 0 { first |= 0x80; }
    let mut result = vec![first];
    while magnitude != 0 {
        let mut next = (magnitude & 0x7f) as u8;
        magnitude >>= 7;
        if magnitude != 0 { next |= 0x80; }
        result.push(next);
    }
    result
}

pub(crate) fn decode_relative_position(encoded: &[u8]) -> Result<(StickyIndex, i32), CodingError> {
    if encoded.is_empty() || encoded.len() > 256 { return Err(CodingError::DecodingError) }
    let position = StickyIndex::decode_v1(encoded).map_err(|_| CodingError::DecodingError)?;
    let canonical = position.encode_v1();
    if canonical.is_empty() || encoded.len() < canonical.len() { return Err(CodingError::DecodingError) }
    let prefix = &canonical[..canonical.len() - 1];
    if !encoded.starts_with(prefix) { return Err(CodingError::DecodingError) }
    let suffix = &encoded[prefix.len()..];
    let mut magnitude = 0u64;
    let mut shift = 0;
    let mut negative = false;
    for (index, byte) in suffix.iter().copied().enumerate() {
        if index == 0 {
            negative = byte & 0x40 != 0;
            magnitude = (byte & 0x3f) as u64;
            shift = 6;
        } else {
            if shift >= 64 { return Err(CodingError::DecodingError) }
            magnitude |= ((byte & 0x7f) as u64) << shift;
            shift += 7;
        }
        if byte & 0x80 == 0 {
            if index + 1 != suffix.len() || magnitude > i32::MAX as u64 + u64::from(negative) {
                return Err(CodingError::DecodingError)
            }
            let value = if negative { -(magnitude as i64) } else { magnitude as i64 } as i32;
            if association_bytes(value) != suffix { return Err(CodingError::DecodingError) }
            return Ok((position, value))
        }
    }
    Err(CodingError::DecodingError)
}

pub(crate) fn relative_position_to_json(encoded: Vec<u8>) -> Result<String, CodingError> {
    let (position, association) = decode_relative_position(&encoded)?;
    let mut json = serde_json::to_value(position).map_err(|_| CodingError::EncodingError)?;
    let object = json.as_object_mut().ok_or(CodingError::EncodingError)?;
    object.insert("assoc".into(), association.into());
    serde_json::to_string(&json).map_err(|_| CodingError::EncodingError)
}

pub(crate) fn relative_position_from_json(json: String) -> Result<Vec<u8>, CodingError> {
    let mut value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|_| CodingError::DecodingError)?;
    let object = value.as_object_mut().ok_or(CodingError::DecodingError)?;
    let association = match object.get("assoc") {
        None | Some(serde_json::Value::Null) => 0,
        Some(value) => value.as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(CodingError::DecodingError)?,
    };
    object.insert("assoc".into(), (if association < 0 { -1 } else { 0 }).into());
    let position: StickyIndex = serde_json::from_value(value)
        .map_err(|_| CodingError::DecodingError)?;
    let mut encoded = position.encode_v1();
    encoded.pop(); // Yrs writes one byte for its normalized association.
    encoded.extend(association_bytes(association));
    if encoded.len() > 256 { return Err(CodingError::EncodingError) }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::{relative_position_from_json, relative_position_to_json};
    use base64::Engine;

    #[test]
    fn yjs_json_and_binary_relative_positions_round_trip() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../Tests/YSwiftTests/Fixtures/relative-position-json-v1.json")).unwrap();
        for entry in fixture["positions"].as_array().unwrap() {
            let encoded = base64::engine::general_purpose::STANDARD
                .decode(entry["binary"].as_str().unwrap()).unwrap();
            let json = serde_json::to_string(&entry["json"]).unwrap();
            assert_eq!(relative_position_from_json(json).unwrap(), encoded);
            let projected: serde_json::Value = serde_json::from_str(&relative_position_to_json(encoded).unwrap()).unwrap();
            assert_eq!(projected, entry["json"]);
        }
        assert!(relative_position_from_json(r#"{"tname":"default","assoc":2147483648}"#.into()).is_err());
        assert!(relative_position_from_json(r#"{"tname":"default","assoc":1.5}"#.into()).is_err());
        assert!(relative_position_to_json(vec![0xff]).is_err());
    }
}
