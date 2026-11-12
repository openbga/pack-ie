//! References to secrets held outside the configuration documents.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The name of a secret in the OS key store or the restricted secrets file. Documents hold
/// only these names; values are resolved at connect time and never logged.
///
/// Written `{"secret": "<name>"}`. Names match `^[a-z0-9][a-z0-9_.-]{0,62}$`. A plain
/// string where a reference belongs is accepted by the parser without keeping its value,
/// and reported by validation as `secret_inline`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, TS)]
pub struct SecretRef {
    /// The secret's name.
    secret: String,
    /// A literal was written instead of a reference (its value is not kept).
    #[serde(skip)]
    #[ts(skip)]
    inline: bool,
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reference {
            secret: String,
        }
        let value = serde_json::Value::deserialize(d)?;
        if value.is_string() {
            return Ok(Self {
                secret: String::new(),
                inline: true,
            });
        }
        let Reference { secret } =
            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            secret,
            inline: false,
        })
    }
}

impl SecretRef {
    /// A reference to the secret `name`, if the name is well formed.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        Self::is_valid_name(&name).then_some(Self {
            secret: name,
            inline: false,
        })
    }

    /// The secret's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.secret
    }

    /// Whether the document held a literal instead of a reference.
    #[must_use]
    pub const fn is_inline(&self) -> bool {
        self.inline
    }

    pub(crate) fn is_valid_name(name: &str) -> bool {
        let mut bytes = name.bytes();
        let first_ok = bytes
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        first_ok
            && name.len() <= 63
            && bytes.all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_restricted() {
        assert!(SecretRef::new("sftp.lis-partner_1").is_some());
        for bad in [
            "",
            "has space",
            "slash/name",
            "Upper",
            "-lead",
            &"x".repeat(64),
        ] {
            assert!(SecretRef::new(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn written_as_an_object() {
        let secret = SecretRef::new("lis-sftp").unwrap();
        assert_eq!(
            serde_json::to_value(&secret).unwrap(),
            serde_json::json!({"secret": "lis-sftp"})
        );
        let inline: SecretRef = serde_json::from_str(r#""hunter2""#).unwrap();
        assert!(inline.is_inline());
        assert!(!format!("{inline:?}").contains("hunter2"));
    }
}
