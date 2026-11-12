use thiserror::Error;

use crate::escape::encode_escapes;
use crate::record_model::Delimiters;

/// Patient fields of a P record. Empty strings stay empty fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatientFields {
    /// Practice-assigned patient identifier (P.3).
    pub practice_id: String,
    /// Laboratory-assigned patient identifier (P.4).
    pub lab_id: String,
    /// Patient name as `last^first` (P.6); the component delimiter is written as is.
    pub name: String,
    /// Birth date text, usually `YYYYMMDD` (P.8).
    pub birth_date: String,
    /// Sex text (P.9).
    pub sex: String,
}

/// Fields of an O record. Empty strings stay empty fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OrderFields {
    /// Specimen identifier (O.3).
    pub specimen_id: String,
    /// Instrument specimen identifier (O.4).
    pub instrument_specimen_id: String,
    /// Test codes, each written as a repeat of the universal test identifier `^^^CODE` (O.5).
    pub test_ids: Vec<String>,
    /// Priority text (O.6).
    pub priority: String,
    /// Requested/ordered date and time text (O.7).
    pub requested_at: String,
    /// Specimen collection date and time text (O.8).
    pub collected_at: String,
    /// Action code, for example `N` for a new order (O.12).
    pub action_code: String,
    /// Specimen descriptor, for example `SERUM` (O.16).
    pub specimen_descriptor: String,
}

/// Fields of a Q record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueryFields {
    /// Patient identifier of the starting range (Q.3.1); empty to omit.
    pub patient_id: String,
    /// Specimen identifier of the starting range (Q.3.2).
    pub specimen_id: String,
    /// Test codes requested; empty means every test (`^^^ALL`) (Q.5).
    pub test_ids: Vec<String>,
    /// Request information status code, for example `O` for orders (Q.13).
    pub status_code: String,
}

impl QueryFields {
    /// A request for the orders of one specimen: status code `O`, all tests.
    #[must_use]
    pub fn for_specimen(specimen_id: impl Into<String>) -> Self {
        Self {
            specimen_id: specimen_id.into(),
            status_code: "O".to_owned(),
            ..Self::default()
        }
    }
}

/// Why a message cannot be built. The text never echoes field values.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BuildError {
    /// The message has neither a patient nor a query.
    #[error("ASTM message to build has no patient or query")]
    EmptyBody,
    /// Patients and queries cannot share one message.
    #[error("ASTM message to build mixes patients and queries")]
    MixedBodyKinds,
    /// A patient without any order.
    #[error("ASTM patient to build has no order")]
    PatientWithoutOrder,
}

struct PatientEntry {
    patient: PatientFields,
    orders: Vec<OrderFields>,
}

/// Builds outbound order downloads (H/P/O/L) and queries (H/Q/L) as unterminated
/// record byte strings, ready for [`crate::LinkDriver::submit`].
///
/// Every value is escaped, so delimiters, control characters and non-ASCII text
/// can never corrupt the record structure. Sequence numbers are assigned per
/// scope. Trailing empty fields are omitted.
pub struct MessageBuilder {
    delimiters: Delimiters,
    sender_name: String,
    sender_version: String,
    receiver_id: String,
    processing_id: String,
    version: String,
    timestamp: String,
    patients: Vec<PatientEntry>,
    queries: Vec<QueryFields>,
}

impl Default for MessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBuilder {
    /// A builder with default delimiters, processing id `P` and version `LIS2-A2`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            delimiters: Delimiters::default(),
            sender_name: String::new(),
            sender_version: String::new(),
            receiver_id: String::new(),
            processing_id: "P".to_owned(),
            version: "LIS2-A2".to_owned(),
            timestamp: String::new(),
            patients: Vec::new(),
            queries: Vec::new(),
        }
    }

    /// Uses a non-default delimiter set announced in the header.
    #[must_use]
    pub const fn delimiters(mut self, delimiters: Delimiters) -> Self {
        self.delimiters = delimiters;
        self
    }

    /// Sets the sender name and version (H.5).
    #[must_use]
    pub fn sender(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.sender_name = name.into();
        self.sender_version = version.into();
        self
    }

    /// Sets the receiver identifier (H.10).
    #[must_use]
    pub fn receiver_id(mut self, receiver_id: impl Into<String>) -> Self {
        self.receiver_id = receiver_id.into();
        self
    }

    /// Sets the processing identifier (H.12), for example `P`, `T` or `D`.
    #[must_use]
    pub fn processing_id(mut self, processing_id: impl Into<String>) -> Self {
        self.processing_id = processing_id.into();
        self
    }

    /// Sets the header timestamp text (H.14), usually `YYYYMMDDHHMMSS`.
    #[must_use]
    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = timestamp.into();
        self
    }

    /// Adds a patient with its orders.
    #[must_use]
    pub fn patient(mut self, patient: PatientFields, orders: Vec<OrderFields>) -> Self {
        self.patients.push(PatientEntry { patient, orders });
        self
    }

    /// Adds a query.
    #[must_use]
    pub fn query(mut self, query: QueryFields) -> Self {
        self.queries.push(query);
        self
    }

    /// Produces the records: H, then the body, then L.
    pub fn build(&self) -> Result<Vec<Vec<u8>>, BuildError> {
        if self.patients.is_empty() && self.queries.is_empty() {
            return Err(BuildError::EmptyBody);
        }
        if !self.patients.is_empty() && !self.queries.is_empty() {
            return Err(BuildError::MixedBodyKinds);
        }
        if self.patients.iter().any(|entry| entry.orders.is_empty()) {
            return Err(BuildError::PatientWithoutOrder);
        }
        let mut records = vec![self.header()];
        for (index, entry) in self.patients.iter().enumerate() {
            records.push(self.patient_record(index + 1, &entry.patient));
            for (order_index, order) in entry.orders.iter().enumerate() {
                records.push(self.order_record(order_index + 1, order));
            }
        }
        for (index, query) in self.queries.iter().enumerate() {
            records.push(self.query_record(index + 1, query));
        }
        records.push(self.record('L', &["1".to_owned(), "N".to_owned()]));
        Ok(records)
    }

    fn escaped(&self, text: &str) -> String {
        encode_escapes(text, self.delimiters)
    }

    fn test_id_field(&self, test_ids: &[String], empty: &str) -> String {
        let repeat = char::from(self.delimiters.repeat());
        let component = char::from(self.delimiters.component());
        if test_ids.is_empty() {
            return format!("{component}{component}{component}{empty}");
        }
        test_ids
            .iter()
            .map(|code| format!("{component}{component}{component}{}", self.escaped(code)))
            .collect::<Vec<_>>()
            .join(&repeat.to_string())
    }

    /// Joins already-encoded fields, dropping trailing empty ones.
    fn record(&self, identifier: char, fields: &[String]) -> Vec<u8> {
        let field_separator = char::from(self.delimiters.field());
        let used = fields
            .iter()
            .rposition(|field| !field.is_empty())
            .map_or(0, |last| last + 1);
        let mut text = String::from(identifier);
        for field in &fields[..used] {
            text.push(field_separator);
            text.push_str(field);
        }
        text.into_bytes()
    }

    fn header(&self) -> Vec<u8> {
        let component = char::from(self.delimiters.component());
        let sender = if self.sender_version.is_empty() {
            self.escaped(&self.sender_name)
        } else {
            format!(
                "{}{component}{}",
                self.escaped(&self.sender_name),
                self.escaped(&self.sender_version)
            )
        };
        let delimiters = format!(
            "{}{}{}",
            char::from(self.delimiters.repeat()),
            component,
            char::from(self.delimiters.escape())
        );
        // Positions after the identifier: 1 delimiters, 4 sender, 9 receiver,
        // 11 processing id, 12 version, 13 timestamp.
        let mut fields = vec![String::new(); 13];
        fields[0] = delimiters;
        fields[3] = sender;
        fields[8] = self.escaped(&self.receiver_id);
        fields[10] = self.escaped(&self.processing_id);
        fields[11] = self.escaped(&self.version);
        fields[12] = self.escaped(&self.timestamp);
        self.record('H', &fields)
    }

    fn patient_record(&self, sequence: usize, patient: &PatientFields) -> Vec<u8> {
        let component = char::from(self.delimiters.component());
        let name = patient
            .name
            .split(component)
            .map(|part| self.escaped(part))
            .collect::<Vec<_>>()
            .join(&component.to_string());
        let mut fields = vec![String::new(); 8];
        fields[0] = sequence.to_string();
        fields[1] = self.escaped(&patient.practice_id);
        fields[2] = self.escaped(&patient.lab_id);
        fields[4] = name;
        fields[6] = self.escaped(&patient.birth_date);
        fields[7] = self.escaped(&patient.sex);
        self.record('P', &fields)
    }

    fn order_record(&self, sequence: usize, order: &OrderFields) -> Vec<u8> {
        let mut fields = vec![String::new(); 15];
        fields[0] = sequence.to_string();
        fields[1] = self.escaped(&order.specimen_id);
        fields[2] = self.escaped(&order.instrument_specimen_id);
        fields[3] = if order.test_ids.is_empty() {
            String::new()
        } else {
            self.test_id_field(&order.test_ids, "")
        };
        fields[4] = self.escaped(&order.priority);
        fields[5] = self.escaped(&order.requested_at);
        fields[6] = self.escaped(&order.collected_at);
        fields[10] = self.escaped(&order.action_code);
        fields[14] = self.escaped(&order.specimen_descriptor);
        self.record('O', &fields)
    }

    fn query_record(&self, sequence: usize, query: &QueryFields) -> Vec<u8> {
        let component = char::from(self.delimiters.component());
        let mut fields = vec![String::new(); 12];
        fields[0] = sequence.to_string();
        fields[1] = format!(
            "{}{component}{}",
            self.escaped(&query.patient_id),
            self.escaped(&query.specimen_id)
        );
        fields[3] = self.test_id_field(&query.test_ids, "ALL");
        fields[11] = self.escaped(&query.status_code);
        self.record('Q', &fields)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::record::parse_message_records;

    fn joined(records: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend_from_slice(record);
            bytes.push(b'\r');
        }
        bytes
    }

    #[test]
    fn order_download_uses_standard_positions() {
        let records = MessageBuilder::new()
            .sender("PACKIE", "1.0")
            .timestamp("20261012120000")
            .patient(
                PatientFields {
                    practice_id: "SYN-PRACTICE".into(),
                    lab_id: "SYN-LAB".into(),
                    name: "Doe^Test".into(),
                    ..PatientFields::default()
                },
                vec![OrderFields {
                    specimen_id: "SYN-0001".into(),
                    test_ids: vec!["SYN-A".into(), "SYN-B".into()],
                    priority: "R".into(),
                    action_code: "N".into(),
                    specimen_descriptor: "SERUM".into(),
                    ..OrderFields::default()
                }],
            )
            .build()
            .expect("synthetic order builds");
        assert_eq!(
            records[0],
            b"H|\\^&|||PACKIE^1.0|||||||P|LIS2-A2|20261012120000".to_vec()
        );
        assert_eq!(records[1], b"P|1|SYN-PRACTICE|SYN-LAB||Doe^Test".to_vec());
        assert_eq!(
            records[2],
            b"O|1|SYN-0001||^^^SYN-A\\^^^SYN-B|R||||||N||||SERUM".to_vec()
        );
        assert_eq!(records[3], b"L|1|N".to_vec());

        let parsed = parse_message_records(&joined(&records)).expect("built message parses");
        let order = parsed[2].as_order().expect("order view");
        assert_eq!(order.specimen_id(), Some("SYN-0001"));
        assert_eq!(order.priority(), Some("R"));
        assert_eq!(order.action_code(), Some("N"));
        assert_eq!(order.specimen_descriptor(), Some("SERUM"));
        assert_eq!(parsed[2].field(4).expect("test field").repeats().len(), 2);
    }

    #[test]
    fn query_for_specimen_round_trips_through_accessors() {
        let records = MessageBuilder::new()
            .sender("PACKIE", "1.0")
            .query(QueryFields::for_specimen("SYN-0002"))
            .build()
            .expect("synthetic query builds");
        assert_eq!(records[1], b"Q|1|^SYN-0002||^^^ALL||||||||O".to_vec());
        let parsed = parse_message_records(&joined(&records)).expect("built query parses");
        let query = parsed[1].as_query().expect("query view");
        assert_eq!(query.specimen_id(), Some("SYN-0002"));
        assert_eq!(query.patient_id(), Some(""));
        assert_eq!(query.status_code(), Some("O"));
        assert_eq!(parsed[1].raw_bytes(), records[1].as_slice());
    }

    #[test]
    fn structural_errors_are_typed() {
        assert_eq!(MessageBuilder::new().build(), Err(BuildError::EmptyBody));
        let mixed = MessageBuilder::new()
            .query(QueryFields::default())
            .patient(PatientFields::default(), vec![OrderFields::default()]);
        assert_eq!(mixed.build(), Err(BuildError::MixedBodyKinds));
        let bare = MessageBuilder::new().patient(PatientFields::default(), Vec::new());
        assert_eq!(bare.build(), Err(BuildError::PatientWithoutOrder));
    }

    #[test]
    fn custom_delimiters_are_used_everywhere() {
        let delimiters = Delimiters::new(b'!', b'@', b'$', b'%').expect("valid delimiters");
        let records = MessageBuilder::new()
            .delimiters(delimiters)
            .sender("PACKIE", "1.0")
            .query(QueryFields::for_specimen("A!B"))
            .build()
            .expect("query builds");
        assert_eq!(records[0], b"H!@$%!!!PACKIE$1.0!!!!!!!P!LIS2-A2".to_vec());
        assert_eq!(records[1], b"Q!1!$A%F%B!!$$$ALL!!!!!!!!O".to_vec());
        let parsed = parse_message_records(&joined(&records)).expect("custom message parses");
        assert_eq!(
            parsed[1]
                .as_query()
                .and_then(|query| query.specimen_id().map(str::to_owned)),
            Some("A!B".to_owned())
        );
    }

    proptest! {
        #[test]
        fn arbitrary_text_never_breaks_record_structure(
            specimen in "\\PC{0,60}",
            descriptor in "\\PC{0,60}",
        ) {
            let records = MessageBuilder::new()
                .sender("PACKIE", "1.0")
                .patient(
                    PatientFields::default(),
                    vec![OrderFields {
                        specimen_id: specimen.clone(),
                        specimen_descriptor: descriptor.clone(),
                        test_ids: vec!["SYN".into()],
                        ..OrderFields::default()
                    }],
                )
                .build()
                .expect("builds");
            prop_assert!(records.iter().all(|record| !record.contains(&b'\r')));
            let parsed = parse_message_records(&joined(&records)).expect("parses");
            let order = parsed[2].as_order().expect("order view");
            prop_assert_eq!(order.specimen_id().unwrap_or(""), specimen.as_str());
            prop_assert_eq!(order.specimen_descriptor().unwrap_or(""), descriptor.as_str());
        }
    }
}
