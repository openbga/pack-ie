use crate::Error;
use std::str::FromStr;

/// A validated generic address. Parse once and reuse with `get_path`/`set_path`.
/// Segments/fields/components/subcomponents are one-based; repetitions zero-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub(crate) segment: String,
    pub(crate) occurrence: usize,
    pub(crate) field: usize,
    pub(crate) repetition: Option<usize>,
    pub(crate) component: Option<usize>,
    pub(crate) subcomponent: Option<usize>,
}

impl Path {
    /// Parse path text such as `OBX(2)-5[1].4.2`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPath`] for bad syntax or a zero one-based index, and
    /// [`Error::PathIndexTooLarge`] for an index above 65535.
    pub fn parse(value: &str) -> Result<Self, Error> {
        value.parse()
    }
    /// Segment name.
    #[must_use]
    pub fn segment(&self) -> &str {
        &self.segment
    }
    /// One-based segment occurrence.
    #[must_use]
    pub fn occurrence(&self) -> usize {
        self.occurrence
    }
    /// One-based field number.
    #[must_use]
    pub fn field(&self) -> usize {
        self.field
    }
    /// Zero-based repetition, when given.
    #[must_use]
    pub fn repetition(&self) -> Option<usize> {
        self.repetition
    }
    /// One-based component number, when given.
    #[must_use]
    pub fn component(&self) -> Option<usize> {
        self.component
    }
    /// One-based subcomponent number, when given.
    #[must_use]
    pub fn subcomponent(&self) -> Option<usize> {
        self.subcomponent
    }
}

impl FromStr for Path {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        let (segment, rest) = value.split_once('-').ok_or(Error::InvalidPath)?;
        let (name, occurrence) = match segment.split_once('(') {
            Some((name, number)) => (
                name,
                index(number.strip_suffix(')').ok_or(Error::InvalidPath)?, false)?,
            ),
            None => (segment, 1),
        };
        if !valid_segment_name(name.as_bytes()) {
            return Err(Error::InvalidPath);
        }
        let mut parts = rest.split('.');
        let first = parts.next().ok_or(Error::InvalidPath)?;
        let (field, repetition) = match first.split_once('[') {
            Some((field, number)) => (
                index(field, false)?,
                Some(index(
                    number.strip_suffix(']').ok_or(Error::InvalidPath)?,
                    true,
                )?),
            ),
            None => (index(first, false)?, None),
        };
        let component = parts.next().map(|part| index(part, false)).transpose()?;
        let subcomponent = parts.next().map(|part| index(part, false)).transpose()?;
        if parts.next().is_some() || (name == "MSH" && occurrence != 1) {
            return Err(Error::InvalidPath);
        }
        Ok(Self {
            segment: name.to_owned(),
            occurrence,
            field,
            repetition,
            component,
            subcomponent,
        })
    }
}

fn index(value: &str, zero_based: bool) -> Result<usize, Error> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidPath);
    }
    let index = value
        .parse::<usize>()
        .map_err(|_| Error::PathIndexTooLarge)?;
    if index > 65535 {
        return Err(Error::PathIndexTooLarge);
    }
    if !zero_based && index == 0 {
        return Err(Error::InvalidPath);
    }
    Ok(index)
}

pub(crate) fn valid_segment_name(name: &[u8]) -> bool {
    name.len() == 3
        && name[0].is_ascii_uppercase()
        && name[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}
