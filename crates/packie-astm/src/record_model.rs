use thiserror::Error;

use crate::accessors::{
    CommentRecord, HeaderRecord, OrderRecord, PatientRecord, QueryRecord, ResultRecord,
};
use crate::ascii::is_printable;

/// Active E1394 field, repeat, component, and escape delimiters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Delimiters {
    field: u8,
    repeat: u8,
    component: u8,
    escape: u8,
}

impl Default for Delimiters {
    fn default() -> Self {
        Self {
            field: b'|',
            repeat: b'\\',
            component: b'^',
            escape: b'&',
        }
    }
}

impl Delimiters {
    /// Constructs a delimiter set, rejecting repeated or non-printable bytes.
    pub fn new(field: u8, repeat: u8, component: u8, escape: u8) -> Result<Self, RecordError> {
        let values = [field, repeat, component, escape];
        let distinct = values
            .iter()
            .enumerate()
            .all(|(index, value)| !values[index + 1..].contains(value));
        if !values.into_iter().all(is_printable) || !distinct {
            return Err(RecordError::InvalidDelimiters);
        }
        Ok(Self {
            field,
            repeat,
            component,
            escape,
        })
    }

    /// Returns the field separator advertised by the header.
    #[must_use]
    pub const fn field(self) -> u8 {
        self.field
    }

    /// Returns the repeat separator advertised by the header.
    #[must_use]
    pub const fn repeat(self) -> u8 {
        self.repeat
    }

    /// Returns the component separator advertised by the header.
    #[must_use]
    pub const fn component(self) -> u8 {
        self.component
    }

    /// Returns the escape separator advertised by the header.
    #[must_use]
    pub const fn escape(self) -> u8 {
        self.escape
    }

    pub(crate) fn from_header(raw: &[u8]) -> Result<Self, RecordError> {
        if raw.len() < 5 || raw[0] != b'H' {
            return Err(RecordError::ExpectedHeader);
        }
        Self::new(raw[1], raw[2], raw[3], raw[4])
    }
}

/// Supported E1394 record identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordType {
    /// Message header.
    Header,
    /// Patient information.
    Patient,
    /// Test order.
    Order,
    /// Test result.
    Result,
    /// Comment attached to the current parent.
    Comment,
    /// Query.
    Query,
    /// Manufacturer information.
    Manufacturer,
    /// Message terminator.
    Terminator,
}

impl RecordType {
    pub(crate) fn parse(byte: u8) -> Result<Self, RecordError> {
        match byte {
            b'H' => Ok(Self::Header),
            b'P' => Ok(Self::Patient),
            b'O' => Ok(Self::Order),
            b'R' => Ok(Self::Result),
            b'C' => Ok(Self::Comment),
            b'Q' => Ok(Self::Query),
            b'M' => Ok(Self::Manufacturer),
            b'L' => Ok(Self::Terminator),
            _ => Err(RecordError::UnknownRecordType),
        }
    }
}

/// One decoded component with its exact encoded spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Component {
    pub(crate) raw: String,
    pub(crate) decoded: String,
}

impl Component {
    /// Returns the exact encoded component text.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns delimiter and hex escapes decoded as text.
    #[must_use]
    pub fn decoded(&self) -> &str {
        &self.decoded
    }
}

/// One repeated value containing zero or more components.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Repeat {
    pub(crate) raw: String,
    pub(crate) components: Vec<Component>,
}

impl Repeat {
    /// Returns the exact encoded repeated-value text.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns components in their transmitted order.
    #[must_use]
    pub fn components(&self) -> &[Component] {
        &self.components
    }
}

/// One positional field containing repeated values and components.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub(crate) raw: String,
    pub(crate) decoded: String,
    pub(crate) repeats: Vec<Repeat>,
}

impl Field {
    /// Returns the exact encoded field text.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns delimiter and hex escapes decoded without parsing a quantity.
    #[must_use]
    pub fn decoded(&self) -> &str {
        &self.decoded
    }

    /// Returns repeated values in their transmitted order.
    #[must_use]
    pub fn repeats(&self) -> &[Repeat] {
        &self.repeats
    }
}

/// A generic supported record with exact raw text and structured fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub(crate) kind: RecordType,
    pub(crate) raw: String,
    pub(crate) fields: Vec<Field>,
}

impl Record {
    /// Returns the parsed record identifier.
    #[must_use]
    pub const fn record_type(&self) -> RecordType {
        self.kind
    }

    /// Returns the complete record text without its terminating carriage return.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the exact record bytes without the terminating carriage return.
    ///
    /// Records are validated printable ASCII, so these are the bytes as received.
    #[must_use]
    pub fn raw_bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }

    /// Returns positional fields after the one-character record identifier.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Returns a one-based positional field after the record identifier.
    #[must_use]
    pub fn field(&self, position: usize) -> Option<&Field> {
        position
            .checked_sub(1)
            .and_then(|index| self.fields.get(index))
    }

    /// Parses field one as a sequence number for sequenced record types.
    pub fn sequence_number(&self) -> Result<Option<u32>, RecordError> {
        if matches!(self.kind, RecordType::Header) {
            return Ok(None);
        }
        let Some(text) = self.field_text(1) else {
            return Ok(None);
        };
        if text.is_empty() {
            return Ok(None);
        }
        text.parse::<u32>()
            .map(Some)
            .map_err(|_| RecordError::InvalidSequenceNumber)
    }

    /// Returns typed header accessors when this is an H record.
    #[must_use]
    pub fn as_header(&self) -> Option<HeaderRecord<'_>> {
        (self.kind == RecordType::Header).then_some(HeaderRecord(self))
    }

    /// Returns typed patient accessors when this is a P record.
    #[must_use]
    pub fn as_patient(&self) -> Option<PatientRecord<'_>> {
        (self.kind == RecordType::Patient).then_some(PatientRecord(self))
    }

    /// Returns typed order accessors when this is an O record.
    #[must_use]
    pub fn as_order(&self) -> Option<OrderRecord<'_>> {
        (self.kind == RecordType::Order).then_some(OrderRecord(self))
    }

    /// Returns typed result accessors when this is an R record.
    #[must_use]
    pub fn as_result(&self) -> Option<ResultRecord<'_>> {
        (self.kind == RecordType::Result).then_some(ResultRecord(self))
    }

    /// Returns typed query accessors when this is a Q record.
    #[must_use]
    pub fn as_query(&self) -> Option<QueryRecord<'_>> {
        (self.kind == RecordType::Query).then_some(QueryRecord(self))
    }

    /// Returns typed comment accessors when this is a C record.
    #[must_use]
    pub fn as_comment(&self) -> Option<CommentRecord<'_>> {
        (self.kind == RecordType::Comment).then_some(CommentRecord(self))
    }

    pub(crate) fn field_text(&self, position: usize) -> Option<&str> {
        self.field(position).map(Field::decoded)
    }

    pub(crate) fn first_components(&self, position: usize) -> Option<&[Component]> {
        self.field(position)
            .and_then(|field| field.repeats.first())
            .map(Repeat::components)
    }
}

/// A record parsing failure whose display text never echoes payload data.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RecordError {
    /// No record bytes were supplied.
    #[error("ASTM record is empty")]
    EmptyRecord,
    /// Record bytes are not printable seven-bit ASCII.
    #[error("ASTM record contains a disallowed byte at offset {offset}")]
    DisallowedByte {
        /// Zero-based byte offset in the record.
        offset: usize,
    },
    /// The record identifier is not supported by this codec.
    #[error("ASTM record type is unsupported")]
    UnknownRecordType,
    /// The first message record is not a header.
    #[error("ASTM message does not start with a header record")]
    ExpectedHeader,
    /// Header delimiter bytes are missing, repeated, or outside printable ASCII.
    #[error("ASTM header delimiters are invalid")]
    InvalidDelimiters,
    /// A delimiter or hex escape is unknown, incomplete, or malformed.
    #[error("ASTM record contains a malformed escape sequence")]
    MalformedEscape,
    /// Decoded hex bytes are not valid UTF-8 text.
    #[error("ASTM hex escape does not decode to UTF-8 text")]
    InvalidHexText,
    /// Message bytes do not end every record with a carriage return.
    #[error("ASTM message contains an unterminated record")]
    MissingRecordTerminator,
    /// A sequence field is present but is not an unsigned decimal integer.
    #[error("ASTM record sequence number is invalid")]
    InvalidSequenceNumber,
}
