//! RFC 8785-style canonical JSON encoding for Unified store documents.
//!
//! Upstream serializes `CURRENT`, manifests and journal records with a
//! canonical JSON encoder: keys sorted by UTF-16 code unit sequences, no
//! whitespace, integers within ±(2^53−1) and escapes matching RFC 8785 for
//! the ASCII control range.  Documents are hashed over these exact bytes, so
//! re-encoding must be byte-identical to what upstream wrote.

use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

use super::error::UnifiedError;

/// Largest integer magnitude representable exactly in IEEE 754 double
/// precision (2^53 − 1); the only integers canonical JSON may carry.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Hashes `bytes` with SHA-256 and returns the digest as lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Encodes `value` in the canonical form used by Unified store documents:
/// no whitespace, object keys sorted by UTF-16 code unit sequences, minimal
/// string escapes, and integers within ±(2^53−1) only.
pub(crate) fn canonical_json(value: &Value) -> Result<Vec<u8>, UnifiedError> {
    let mut out = Vec::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

/// Parses canonical JSON: `body` must be valid JSON whose canonical
/// re-encoding is byte-identical, otherwise the document is rejected as
/// [`UnifiedError::Corrupt`].
pub(crate) fn parse_canonical(body: &[u8]) -> Result<Value, UnifiedError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| UnifiedError::Corrupt(format!("document is not valid JSON: {error}")))?;
    let reencoded = canonical_json(&value)?;
    if reencoded != body {
        return Err(UnifiedError::Corrupt(
            "document is not canonical".to_string(),
        ));
    }
    Ok(value)
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), UnifiedError> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => write_number(number, out)?,
        Value::String(string) => write_string(string, out),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push(b'{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_string(key, out);
                out.push(b':');
                write_canonical(item, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn write_number(number: &Number, out: &mut Vec<u8>) -> Result<(), UnifiedError> {
    match number.as_i64() {
        Some(value) if value.unsigned_abs() <= MAX_SAFE_INTEGER => {
            out.extend_from_slice(value.to_string().as_bytes());
            Ok(())
        }
        _ => Err(UnifiedError::Unsupported(
            "non-integer number in canonical document".to_string(),
        )),
    }
}

fn write_string(value: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in value.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            ch if (ch as u32) < 0x20 => {
                let escaped = format!("\\u{:04x}", ch as u32);
                out.extend_from_slice(escaped.as_bytes());
            }
            ch => {
                let mut buffer = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_of_empty_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn canonical_sorts_keys_by_utf16_code_units() {
        // U+1F600 (surrogates 0xD83D…) sorts before U+E000 (0xE000) in UTF-16
        // order, the opposite of UTF-8 byte order.
        let v = serde_json::json!({"\u{1F600}": 1, "\u{E000}": 2, "a": 3});
        assert_eq!(
            canonical_json(&v).unwrap(),
            "{\"a\":3,\"\u{1F600}\":1,\"\u{E000}\":2}".as_bytes()
        );
    }

    #[test]
    fn canonical_escapes_like_rfc8785() {
        let v = serde_json::json!({"s": "q\"b\\n\n\t\u{1}/é\u{7f}"});
        assert_eq!(
            canonical_json(&v).unwrap(),
            br#"{"s":"q\"b\\n\n\t\u0001/"#
                .iter()
                .copied()
                .chain("é\u{7f}\"}".bytes())
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn canonical_rejects_non_integer_and_unsafe_numbers() {
        assert!(canonical_json(&serde_json::json!({"n": 1.5})).is_err());
        assert!(canonical_json(&serde_json::json!({"n": 9007199254740992u64})).is_err());
        assert!(canonical_json(&serde_json::json!({"n": -9007199254740991i64})).is_ok());
    }

    #[test]
    fn parse_canonical_rejects_whitespace_duplicates_and_float_spelling() {
        assert!(parse_canonical(br#"{"a":1}"#).is_ok());
        assert!(parse_canonical(br#"{"a": 1}"#).is_err());
        assert!(parse_canonical(br#"{"a":1,"a":1}"#).is_err());
        assert!(parse_canonical(br#"{"a":1.0}"#).is_err());
        assert!(parse_canonical(br#"{"b":1,"a":2}"#).is_err());
    }
}
