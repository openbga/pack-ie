use crate::record_model::{Component, Record, RecordError};

/// Typed access to header fields while retaining the generic record.
#[derive(Clone, Copy, Debug)]
pub struct HeaderRecord<'a>(pub(crate) &'a Record);

impl HeaderRecord<'_> {
    /// Returns the sender-name component of header field 4 (H.5 including type).
    #[must_use]
    pub fn sender_name(&self) -> Option<&str> {
        component_text(self.0, 4, 0)
    }

    /// Returns the sender-version component of header field 4 (H.5 including type).
    #[must_use]
    pub fn sender_version(&self) -> Option<&str> {
        component_text(self.0, 4, 1)
    }

    /// Returns the processing identifier in header field 11.
    #[must_use]
    pub fn processing_id(&self) -> Option<&str> {
        self.0.field_text(11)
    }

    /// Returns the protocol version text in header field 12.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.0.field_text(12)
    }

    /// Returns the source timestamp text in header field 13 without interpretation.
    #[must_use]
    pub fn timestamp_text(&self) -> Option<&str> {
        self.0.field_text(13)
    }
}

/// Typed access to patient fields while retaining the generic record.
#[derive(Clone, Copy, Debug)]
pub struct PatientRecord<'a>(pub(crate) &'a Record);

impl PatientRecord<'_> {
    /// Returns the validated sequence number when present.
    pub fn sequence(&self) -> Result<Option<u32>, RecordError> {
        self.0.sequence_number()
    }

    /// Returns the practice-assigned patient identifier text.
    #[must_use]
    pub fn practice_assigned_id(&self) -> Option<&str> {
        self.0.field_text(2)
    }

    /// Returns the laboratory-assigned patient identifier text.
    #[must_use]
    pub fn laboratory_assigned_id(&self) -> Option<&str> {
        self.0.field_text(3)
    }
}

/// Typed access to order fields while retaining the generic record.
#[derive(Clone, Copy, Debug)]
pub struct OrderRecord<'a>(pub(crate) &'a Record);

impl OrderRecord<'_> {
    /// Returns the validated sequence number when present.
    pub fn sequence(&self) -> Result<Option<u32>, RecordError> {
        self.0.sequence_number()
    }

    /// Returns the specimen identifier text.
    #[must_use]
    pub fn specimen_id(&self) -> Option<&str> {
        self.0.field_text(2)
    }

    /// Returns the instrument specimen identifier text.
    #[must_use]
    pub fn instrument_specimen_id(&self) -> Option<&str> {
        self.0.field_text(3)
    }

    /// Returns universal-test-identifier components without interpretation.
    #[must_use]
    pub fn universal_test_id_components(&self) -> Option<&[Component]> {
        self.0.first_components(4)
    }

    /// Returns the priority text.
    #[must_use]
    pub fn priority(&self) -> Option<&str> {
        self.0.field_text(5)
    }

    /// Returns the action-code text.
    #[must_use]
    pub fn action_code(&self) -> Option<&str> {
        self.0.field_text(11)
    }

    /// Returns the specimen-descriptor text.
    #[must_use]
    pub fn specimen_descriptor(&self) -> Option<&str> {
        self.0.field_text(15)
    }
}

/// Typed access to result fields; result values always remain text.
#[derive(Clone, Copy, Debug)]
pub struct ResultRecord<'a>(pub(crate) &'a Record);

impl ResultRecord<'_> {
    /// Returns the validated sequence number when present.
    pub fn sequence(&self) -> Result<Option<u32>, RecordError> {
        self.0.sequence_number()
    }

    /// Returns universal-test-identifier components without interpretation.
    #[must_use]
    pub fn universal_test_id_components(&self) -> Option<&[Component]> {
        self.0.first_components(2)
    }

    /// Returns the generic record, including its exact raw text.
    #[must_use]
    pub const fn record(&self) -> &Record {
        self.0
    }

    /// Returns the local test code: the fourth component of the universal test
    /// identifier (`^^^CODE`), the position analyzers conventionally use.
    #[must_use]
    pub fn test_code(&self) -> Option<&str> {
        component_text(self.0, 2, 3)
    }

    /// Returns the exact decoded result-value text without numeric conversion.
    #[must_use]
    pub fn value_text(&self) -> Option<&str> {
        self.0.field_text(3)
    }

    /// Returns the units text.
    #[must_use]
    pub fn units(&self) -> Option<&str> {
        self.0.field_text(4)
    }

    /// Returns the reference-range text without parsing quantities.
    #[must_use]
    pub fn reference_range_text(&self) -> Option<&str> {
        self.0.field_text(5)
    }

    /// Returns the abnormal-flags text.
    #[must_use]
    pub fn abnormal_flags(&self) -> Option<&str> {
        self.0.field_text(6)
    }

    /// Returns the result-status text.
    #[must_use]
    pub fn result_status(&self) -> Option<&str> {
        self.0.field_text(8)
    }

    /// Returns the operator text asserted by the source.
    #[must_use]
    pub fn operator(&self) -> Option<&str> {
        self.0.field_text(10)
    }

    /// Returns the source completion timestamp text without interpretation.
    #[must_use]
    pub fn completion_timestamp_text(&self) -> Option<&str> {
        self.0.field_text(12)
    }

    /// Returns the instrument identifier text.
    #[must_use]
    pub fn instrument_id(&self) -> Option<&str> {
        self.0.field_text(13)
    }
}

/// Typed access to query fields while retaining the generic record.
#[derive(Clone, Copy, Debug)]
pub struct QueryRecord<'a>(pub(crate) &'a Record);

impl QueryRecord<'_> {
    /// Returns the validated sequence number when present.
    pub fn sequence(&self) -> Result<Option<u32>, RecordError> {
        self.0.sequence_number()
    }

    /// Returns the generic record, including its exact raw text.
    #[must_use]
    pub const fn record(&self) -> &Record {
        self.0
    }

    /// Returns the starting-range identifier components (`patient^specimen`).
    #[must_use]
    pub fn starting_range_components(&self) -> Option<&[Component]> {
        self.0.first_components(2)
    }

    /// Returns the patient identifier of the starting range, when sent.
    #[must_use]
    pub fn patient_id(&self) -> Option<&str> {
        component_text(self.0, 2, 0)
    }

    /// Returns the specimen identifier of the starting range, when sent.
    #[must_use]
    pub fn specimen_id(&self) -> Option<&str> {
        component_text(self.0, 2, 1)
    }

    /// Returns universal-test-identifier components without interpretation.
    #[must_use]
    pub fn universal_test_id_components(&self) -> Option<&[Component]> {
        self.0.first_components(4)
    }

    /// Returns the request information status code (for example `O`).
    #[must_use]
    pub fn status_code(&self) -> Option<&str> {
        self.0.field_text(12)
    }
}

/// Typed access to comment fields while retaining exact raw text.
#[derive(Clone, Copy, Debug)]
pub struct CommentRecord<'a>(pub(crate) &'a Record);

impl CommentRecord<'_> {
    /// Returns the comment source text.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.0.field_text(2)
    }

    /// Returns decoded comment text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.0.field_text(3)
    }

    /// Returns the comment-type text.
    #[must_use]
    pub fn comment_type(&self) -> Option<&str> {
        self.0.field_text(4)
    }
}

fn component_text(record: &Record, field: usize, component: usize) -> Option<&str> {
    record
        .first_components(field)
        .and_then(|components| components.get(component))
        .map(Component::decoded)
}
