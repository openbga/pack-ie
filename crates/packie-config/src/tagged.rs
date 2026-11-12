//! Deserialization of `kind`-tagged enums that keeps field paths.
//!
//! serde's internally tagged enums buffer their content, so `serde_path_to_error` stops
//! at the enum (`channels[0]`). These enums instead deserialize the variant body with
//! their own path tracking and carry the inner path inside the error message, marked
//! with [`MARK`]; [`into_validation_error`] joins outer and inner paths again.

use serde::Deserializer;
use serde::de::{DeserializeOwned, Error as _};
use serde_json::{Map, Value};

use crate::validate::{Problem, ProblemCode};

const MARK: char = '\u{1}';

/// Deserializes the body `map` (tag already removed) as `T`: a variant's struct, or the
/// fields next to a kind and its settings.
pub(crate) fn variant<'de, D, T>(map: Map<String, Value>) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    at::<D, T>(Value::Object(map), "")
}

/// Removes the string `tag` and the `content` value (an empty object when absent) from
/// `map`, for documents that put a kind and its settings next to common fields.
pub(crate) fn split_kind<'de, D: Deserializer<'de>>(
    map: &mut Map<String, Value>,
    tag: &'static str,
    content: &'static str,
) -> Result<(String, Value), D::Error> {
    let tag_value = match map.remove(tag) {
        Some(Value::String(text)) => text,
        Some(_) => return Err(D::Error::custom(nest(tag, "must be a string"))),
        None => return Err(D::Error::missing_field(tag)),
    };
    let content_value = map
        .remove(content)
        .unwrap_or_else(|| Value::Object(Map::new()));
    Ok((tag_value, content_value))
}

/// Deserializes `value` as `T`, reporting failures under `path`.
pub(crate) fn at<'de, D, T>(value: Value, path: &str) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    serde_path_to_error::deserialize(value).map_err(|err| {
        let inner = err.path().to_string();
        let inner = nest(&inner, &err.into_inner().to_string());
        D::Error::custom(nest(path, &inner))
    })
}

/// An unknown value of `field`.
pub(crate) fn unknown<'de, D: Deserializer<'de>>(
    field: &str,
    value: &str,
    expected: &[&str],
) -> D::Error {
    D::Error::custom(nest(
        field,
        &format!(
            "unknown {field} `{value}`, expected one of {}",
            expected.join(", ")
        ),
    ))
}

/// Converts a `serde_path_to_error` failure into a problem with the full path. Malformed
/// CIDR blocks get their own code.
pub(crate) fn into_problem(path: &str, message: &str) -> Problem {
    let (inner, message) = split(message);
    let code = if message.contains(crate::endpoint::CIDR_ERROR) {
        ProblemCode::InvalidCidr
    } else {
        ProblemCode::Schema
    };
    // serde stops at the first schema error, so later problems surface only once this one
    // is fixed.
    let message = format!(
        "{message}. Fix this and validate again; later problems are reported once it is fixed"
    );
    Problem::error(code, join(path, inner), message)
}

fn nest(path: &str, message: &str) -> String {
    let (inner, message) = split(message);
    format!("{MARK}{}{MARK}{message}", join(path, inner))
}

fn split(message: &str) -> (&str, &str) {
    message
        .strip_prefix(MARK)
        .and_then(|rest| rest.split_once(MARK))
        .unwrap_or(("", message))
}

fn join(outer: &str, inner: &str) -> String {
    let outer = if outer == "." { "" } else { outer };
    let inner = if inner == "." { "" } else { inner };
    match (outer.is_empty(), inner.is_empty()) {
        (_, true) => outer.to_owned(),
        (true, false) => inner.to_owned(),
        (false, false) if inner.starts_with('[') => format!("{outer}{inner}"),
        (false, false) => format!("{outer}.{inner}"),
    }
}

/// Implements `Deserialize` for an enum tagged by `$tag`, whose variants wrap one struct
/// each, matching its `#[serde(tag = ..)]` serialization.
macro_rules! tagged_deserialize {
    (
        $ty:ident, $tag:literal,
        { $($name:literal => $variant:ident($inner:ty)),* $(,)? }
    ) => {
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                use serde::de::Error as _;
                let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
                let tag = match map.remove($tag) {
                    Some(serde_json::Value::String(tag)) => tag,
                    Some(_) => return Err(D::Error::custom(concat!("`", $tag, "` must be a string"))),
                    None => return Err(D::Error::missing_field($tag)),
                };
                match tag.as_str() {
                    $($name => $crate::tagged::variant::<D, $inner>(map).map(Self::$variant),)*
                    other => Err(D::Error::unknown_variant(
                        other,
                        &[$($name),*],
                    )),
                }
            }
        }
    };
}

pub(crate) use tagged_deserialize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_across_levels() {
        let inner = nest("pairing.stratgy", "unknown field");
        let outer = nest("source", &nest("[0]", &inner));
        let error = into_problem("channels[1]", &format!("{outer} at line 1"));
        assert_eq!(error.path, "channels[1].source[0].pairing.stratgy");
        assert!(
            error
                .message
                .starts_with("unknown field at line 1. Fix this"),
            "{}",
            error.message
        );
        let plain = into_problem(".", "expected value");
        assert_eq!(plain.path, "");
    }
}
