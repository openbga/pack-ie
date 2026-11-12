use thiserror::Error;

use crate::record_model::{Record, RecordError, RecordType};

/// One result record and comments that immediately follow it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultNode {
    record: Record,
    comments: Vec<Record>,
}

impl ResultNode {
    /// Returns the result record.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }

    /// Returns comments attached to this result.
    #[must_use]
    pub fn comments(&self) -> &[Record] {
        &self.comments
    }
}

/// One order record with order-level comments and results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderNode {
    record: Record,
    comments: Vec<Record>,
    results: Vec<ResultNode>,
}

impl OrderNode {
    /// Returns the order record.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }

    /// Returns comments attached before the first result.
    #[must_use]
    pub fn comments(&self) -> &[Record] {
        &self.comments
    }

    /// Returns results in source sequence.
    #[must_use]
    pub fn results(&self) -> &[ResultNode] {
        &self.results
    }
}

/// One patient record with patient-level comments and orders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatientNode {
    record: Record,
    comments: Vec<Record>,
    orders: Vec<OrderNode>,
}

impl PatientNode {
    /// Returns the patient record.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }

    /// Returns comments attached before the first order.
    #[must_use]
    pub fn comments(&self) -> &[Record] {
        &self.comments
    }

    /// Returns orders in source sequence.
    #[must_use]
    pub fn orders(&self) -> &[OrderNode] {
        &self.orders
    }
}

/// A structurally validated result, query, or manufacturer message tree.
///
/// Result messages use H/P/O/R with C attached to the nearest P, O, or R.
/// Query and manufacturer records are supported as top-level alternatives;
/// dialect-specific C or M placements fail with a typed hierarchy error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    header: Record,
    patients: Vec<PatientNode>,
    queries: Vec<Record>,
    manufacturers: Vec<Record>,
    terminator: Record,
}

impl Message {
    /// Returns the leading header record.
    #[must_use]
    pub const fn header(&self) -> &Record {
        &self.header
    }

    /// Returns patient subtrees in source sequence.
    #[must_use]
    pub fn patients(&self) -> &[PatientNode] {
        &self.patients
    }

    /// Returns top-level query records in source sequence.
    #[must_use]
    pub fn queries(&self) -> &[Record] {
        &self.queries
    }

    /// Returns top-level manufacturer records in source sequence.
    #[must_use]
    pub fn manufacturers(&self) -> &[Record] {
        &self.manufacturers
    }

    /// Returns the final terminator record.
    #[must_use]
    pub const fn terminator(&self) -> &Record {
        &self.terminator
    }
}

/// A hierarchy or sequence failure that never echoes record content.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MessageError {
    /// No records were supplied.
    #[error("ASTM message has no records")]
    Empty,
    /// The first record is not H.
    #[error("ASTM message does not start with H")]
    MissingHeader,
    /// The last record is not L.
    #[error("ASTM message does not end with L")]
    MissingTerminator,
    /// H or L appeared inside the message body.
    #[error("ASTM header or terminator appears inside the message body")]
    MisplacedBoundary,
    /// A patient-dependent record appeared before a patient.
    #[error("ASTM record requires a preceding patient record")]
    MissingPatient,
    /// A result-dependent record appeared before an order.
    #[error("ASTM record requires a preceding order record")]
    MissingOrder,
    /// The message mixes a query/manufacturer body with patient results.
    #[error("ASTM top-level and patient-result bodies cannot be mixed")]
    MixedBodyKinds,
    /// The message has no query, manufacturer, or patient body.
    #[error("ASTM message body is empty")]
    EmptyBody,
    /// A patient has no order.
    #[error("ASTM patient has no order")]
    PatientWithoutOrder,
    /// An order has no result.
    #[error("ASTM order has no result")]
    OrderWithoutResult,
    /// A required sequence field is absent.
    #[error("ASTM sequenced record is missing its sequence number")]
    MissingSequence,
    /// A sequence does not equal the next expected value in its scope.
    #[error("ASTM record sequence is out of order")]
    OutOfSequence,
    /// The sequence field is malformed.
    #[error(transparent)]
    Record(#[from] RecordError),
}

/// Validates the supported hierarchy and scoped sequences, then builds a tree.
pub fn assemble_message(mut records: Vec<Record>) -> Result<Message, MessageError> {
    if records.is_empty() {
        return Err(MessageError::Empty);
    }
    if records[0].record_type() != RecordType::Header {
        return Err(MessageError::MissingHeader);
    }
    if records.last().map(Record::record_type) != Some(RecordType::Terminator) {
        return Err(MessageError::MissingTerminator);
    }

    let header = records.remove(0);
    let terminator = records.pop().ok_or(MessageError::MissingTerminator)?;
    validate_terminator_sequence(&terminator)?;
    let mut message = Message {
        header,
        patients: Vec::new(),
        queries: Vec::new(),
        manufacturers: Vec::new(),
        terminator,
    };
    for record in records {
        add_body_record(&mut message, record)?;
    }
    validate_complete_body(&message)?;
    Ok(message)
}

fn add_body_record(message: &mut Message, record: Record) -> Result<(), MessageError> {
    match record.record_type() {
        RecordType::Header | RecordType::Terminator => Err(MessageError::MisplacedBoundary),
        RecordType::Patient => add_patient(message, record),
        RecordType::Order => add_order(message, record),
        RecordType::Result => add_result(message, record),
        RecordType::Comment => add_comment(message, record),
        RecordType::Query | RecordType::Manufacturer => add_top_level(message, record),
    }
}

fn add_patient(message: &mut Message, record: Record) -> Result<(), MessageError> {
    if !message.queries.is_empty() || !message.manufacturers.is_empty() {
        return Err(MessageError::MixedBodyKinds);
    }
    require_sequence(&record, message.patients.len() + 1)?;
    message.patients.push(PatientNode {
        record,
        comments: Vec::new(),
        orders: Vec::new(),
    });
    Ok(())
}

fn add_order(message: &mut Message, record: Record) -> Result<(), MessageError> {
    let patient = message
        .patients
        .last_mut()
        .ok_or(MessageError::MissingPatient)?;
    require_sequence(&record, patient.orders.len() + 1)?;
    patient.orders.push(OrderNode {
        record,
        comments: Vec::new(),
        results: Vec::new(),
    });
    Ok(())
}

fn add_result(message: &mut Message, record: Record) -> Result<(), MessageError> {
    let order = current_order_mut(message)?;
    require_sequence(&record, order.results.len() + 1)?;
    order.results.push(ResultNode {
        record,
        comments: Vec::new(),
    });
    Ok(())
}

fn add_comment(message: &mut Message, record: Record) -> Result<(), MessageError> {
    let patient = message
        .patients
        .last_mut()
        .ok_or(MessageError::MissingPatient)?;
    let Some(order) = patient.orders.last_mut() else {
        require_sequence(&record, patient.comments.len() + 1)?;
        patient.comments.push(record);
        return Ok(());
    };
    let Some(result) = order.results.last_mut() else {
        require_sequence(&record, order.comments.len() + 1)?;
        order.comments.push(record);
        return Ok(());
    };
    require_sequence(&record, result.comments.len() + 1)?;
    result.comments.push(record);
    Ok(())
}

fn add_top_level(message: &mut Message, record: Record) -> Result<(), MessageError> {
    if !message.patients.is_empty() {
        return Err(MessageError::MixedBodyKinds);
    }
    let target = match record.record_type() {
        RecordType::Query => &mut message.queries,
        RecordType::Manufacturer => &mut message.manufacturers,
        RecordType::Header
        | RecordType::Patient
        | RecordType::Order
        | RecordType::Result
        | RecordType::Comment
        | RecordType::Terminator => return Err(MessageError::MisplacedBoundary),
    };
    require_sequence(&record, target.len() + 1)?;
    target.push(record);
    Ok(())
}

fn current_order_mut(message: &mut Message) -> Result<&mut OrderNode, MessageError> {
    message
        .patients
        .last_mut()
        .ok_or(MessageError::MissingPatient)?
        .orders
        .last_mut()
        .ok_or(MessageError::MissingOrder)
}

fn require_sequence(record: &Record, expected: usize) -> Result<(), MessageError> {
    let actual = record
        .sequence_number()?
        .ok_or(MessageError::MissingSequence)?;
    let expected = u32::try_from(expected).map_err(|_| MessageError::OutOfSequence)?;
    if actual == expected {
        Ok(())
    } else {
        Err(MessageError::OutOfSequence)
    }
}

fn validate_terminator_sequence(record: &Record) -> Result<(), MessageError> {
    let sequence = record
        .sequence_number()?
        .ok_or(MessageError::MissingSequence)?;
    if sequence != 1 {
        return Err(MessageError::OutOfSequence);
    }
    Ok(())
}

fn validate_complete_body(message: &Message) -> Result<(), MessageError> {
    if message.patients.is_empty() {
        if message.queries.is_empty() && message.manufacturers.is_empty() {
            return Err(MessageError::EmptyBody);
        }
        return Ok(());
    }
    for patient in &message.patients {
        if patient.orders.is_empty() {
            return Err(MessageError::PatientWithoutOrder);
        }
        if patient.orders.iter().any(|order| order.results.is_empty()) {
            return Err(MessageError::OrderWithoutResult);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::record::parse_message_records;

    fn parse(bytes: &[u8]) -> Vec<Record> {
        parse_message_records(bytes).expect("synthetic records parse")
    }

    #[test]
    fn builds_multiple_patient_order_result_subtrees() {
        let records = parse(b"H|\\^&|||SYNTH-ANALYZER^1.0||||||P|LIS2-A2|20261012120000\rP|1|SYN-PRACTICE-1|SYN-LAB-1\rO|1|SYN-0001||^^^SYN-A|||||||N|||SERUM\rR|1|^^^SYN-A|1.2300|ng/mL||||F\rC|1|I|Synthetic comment|G\rP|2|SYN-PRACTICE-2|SYN-LAB-2\rO|1|SYN-0002||^^^SYN-B|||||||N|||URINE\rR|1|^^^SYN-B|NEGATIVE|||||F\rL|1|N\r");
        let message = assemble_message(records).expect("message hierarchy is valid");
        assert_eq!(message.patients().len(), 2);
        assert_eq!(message.patients()[0].orders()[0].results().len(), 1);
        assert_eq!(
            message.patients()[0].orders()[0].results()[0]
                .comments()
                .len(),
            1
        );
    }

    #[test]
    fn missing_terminator_is_rejected() {
        let records = parse(b"H|\\^&|||SYNTH-ANALYZER^1.0\rP|1|SYN-PRACTICE-1|SYN-LAB-1\r");
        assert_eq!(
            assemble_message(records),
            Err(MessageError::MissingTerminator)
        );
    }

    #[test]
    fn out_of_sequence_result_is_rejected() {
        let records = parse(b"H|\\^&|||SYNTH-ANALYZER^1.0\rP|1|SYN-PRACTICE-1|SYN-LAB-1\rO|1|SYN-0001\rR|2|^^^SYN-A|1.0\rL|1|N\r");
        assert_eq!(assemble_message(records), Err(MessageError::OutOfSequence));
    }

    #[test]
    fn terminator_sequence_is_required() {
        let records = parse(b"H|\\^&|||SYNTH-ANALYZER^1.0\rP|1|SYN-PRACTICE-1|SYN-LAB-1\rO|1|SYN-0001\rR|1|^^^SYN-A|1.0\rL\r");
        assert_eq!(
            assemble_message(records),
            Err(MessageError::MissingSequence)
        );
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_through_record_and_message_parser_never_panic(
            bytes in prop::collection::vec(any::<u8>(), 0..2048),
        ) {
            if let Ok(records) = parse_message_records(&bytes) {
                let _ = assemble_message(records);
            }
        }
    }
}
