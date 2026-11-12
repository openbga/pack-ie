//! The hash chain: canonical encoding of an event and its SHA-256 link.
//!
//! `hash = hex(SHA-256(prev_hash || encoding))`, where `prev_hash` is the previous event's
//! hash as 64 lowercase hex characters (64 zeros before the first event) and `encoding` is
//! the fields below in this order, each as one byte `0` (absent) or `1` (present) followed,
//! when present, by its UTF-8 length as a 4-byte big-endian integer and the bytes:
//! `seq` (decimal), `at` (as stored), `actor_uid`, `actor_name`, `origin`, `action`,
//! `object_type`, `object_id`, `outcome`, `details` (as stored: compact JSON with object
//! keys sorted). Anyone can recompute it from the stored columns (`docs/spec/audit.md` §4).

use sha2::{Digest, Sha256};

/// The `prev_hash` of the first event.
pub(crate) const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The stored columns that the hash covers.
pub(crate) struct Fields<'a> {
    pub(crate) seq: u64,
    pub(crate) at: &'a str,
    pub(crate) actor_uid: &'a str,
    pub(crate) actor_name: &'a str,
    pub(crate) origin: &'a str,
    pub(crate) action: &'a str,
    pub(crate) object_type: Option<&'a str>,
    pub(crate) object_id: Option<&'a str>,
    pub(crate) outcome: &'a str,
    pub(crate) details: &'a str,
}

/// The hash of `fields` linked to `prev_hash`.
pub(crate) fn hash(prev_hash: &str, fields: &Fields<'_>) -> String {
    let mut digest = Sha256::new();
    digest.update(prev_hash.as_bytes());
    let seq = fields.seq.to_string();
    for value in [
        Some(seq.as_str()),
        Some(fields.at),
        Some(fields.actor_uid),
        Some(fields.actor_name),
        Some(fields.origin),
        Some(fields.action),
        fields.object_type,
        fields.object_id,
        Some(fields.outcome),
        Some(fields.details),
    ] {
        match value {
            None => digest.update([0u8]),
            Some(text) => {
                digest.update([1u8]);
                // Fields are bounded far below 4 GiB (details at most 4 KiB).
                let length = u32::try_from(text.len()).unwrap_or(u32::MAX);
                digest.update(length.to_be_bytes());
                digest.update(text.as_bytes());
            }
        }
    }
    hex(&digest.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Compact JSON with every object's keys sorted, so equal details always hash equally.
pub(crate) fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(item) = map.get(key) {
                    write_canonical(item, out);
                }
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(details: &str) -> Fields<'_> {
        Fields {
            seq: 1,
            at: "2026-11-06T12:00:00.000000000Z",
            actor_uid: "501",
            actor_name: "jdoe",
            origin: "cli",
            action: "channel_pause",
            object_type: Some("channel"),
            object_id: Some("lis"),
            outcome: "succeeded",
            details,
        }
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let value = serde_json::json!({"b": 1, "a": {"y": [true, null], "x": "é\""}});
        assert_eq!(
            canonical_json(&value),
            r#"{"a":{"x":"é\"","y":[true,null]},"b":1}"#
        );
    }

    #[test]
    fn every_field_and_the_link_change_the_hash() {
        let base = hash(GENESIS, &fields("{}"));
        assert_eq!(base.len(), 64);
        assert_eq!(base, hash(GENESIS, &fields("{}")));
        assert_ne!(base, hash(&base, &fields("{}")));
        assert_ne!(base, hash(GENESIS, &fields(r#"{"a":1}"#)));
        let mut moved = fields("{}");
        moved.object_type = None;
        moved.object_id = Some("channel");
        assert_ne!(base, hash(GENESIS, &moved), "absent and present differ");
        let mut shifted = fields("{}");
        shifted.actor_uid = "50";
        shifted.actor_name = "1jdoe";
        assert_ne!(base, hash(GENESIS, &shifted), "lengths delimit fields");
    }
}
