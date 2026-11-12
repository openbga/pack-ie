//! AU adapter over [`packie_au::SampleResults`] (read-only).
//!
//! Paths use the protocol's field names. Header fields are written bare (`sample_id`);
//! each result item is a `test` occurrence (`test[0].code`, `test[1].value`).
//!
//! | Name | Content |
//! | --- | --- |
//! | `kind` | `normal`, `rerun`, `stat`, `stat_rerun`, `reagent_blank`, `calibration`, `quality_control` |
//! | `unit_no`, `rack`, `cup`, `sample_type`, `sample_no`, `sample_id`, `reserved` | the fixed-width sample header fields |
//! | `sex`, `age`, `month`, `info1` to `info6` | demographics of normal patient results |
//! | `test[n].code`, `.value`, `.flags` | result items in block order |
//!
//! Fixed-width padding is trimmed; leading zeroes are kept. A header field the
//! instrument profile does not carry is absent (empty text).

use std::borrow::Cow;
use std::collections::BTreeMap;

use packie_au::{ResultKind, SampleResults};

use crate::error::ModelError;
use crate::model::Model;
use crate::path::{FieldRef, Path, Syntax};

/// Header field names, in documentation order.
pub(crate) const HEADER_FIELDS: [&str; 17] = [
    "kind",
    "unit_no",
    "rack",
    "cup",
    "sample_type",
    "sample_no",
    "sample_id",
    "reserved",
    "sex",
    "age",
    "month",
    "info1",
    "info2",
    "info3",
    "info4",
    "info5",
    "info6",
];

/// Result item field names.
pub(crate) const TEST_FIELDS: [&str; 3] = ["code", "value", "flags"];

/// A completed AU sample, read through [`Model`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuModel {
    header: BTreeMap<&'static str, String>,
    tests: Vec<[String; 3]>,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

const fn kind_name(kind: ResultKind) -> &'static str {
    match kind {
        ResultKind::Normal => "normal",
        ResultKind::Rerun => "rerun",
        ResultKind::Stat => "stat",
        ResultKind::StatRerun => "stat_rerun",
        ResultKind::ReagentBlank => "reagent_blank",
        ResultKind::Calibration => "calibration",
        ResultKind::QualityControl => "quality_control",
    }
}

impl AuModel {
    /// Reads the fields of a sample whose blocks have all been received.
    #[must_use]
    pub fn from_results(results: &SampleResults) -> Self {
        let header = &results.header;
        let mut fields = BTreeMap::new();
        fields.insert("kind", kind_name(results.kind).to_owned());
        if let Some(unit) = &header.unit_no {
            fields.insert("unit_no", text(unit.expose()));
        }
        fields.insert("rack", text(header.rack.expose()));
        fields.insert("cup", text(header.cup.expose()));
        fields.insert("sample_type", text(header.sample_type.expose()));
        fields.insert("sample_no", text(header.sample_no.expose()));
        fields.insert("sample_id", text(header.sample_id.expose()));
        fields.insert("reserved", text(header.reserved.expose()));
        if let Some(d) = &header.demographics {
            fields.insert("sex", text(d.sex.expose()));
            fields.insert("age", text(d.age.expose()));
            fields.insert("month", text(d.month.expose()));
            let information = HEADER_FIELDS.into_iter().filter(|n| n.starts_with("info"));
            for (name, field) in information.zip(&d.information) {
                fields.insert(name, text(field.expose()));
            }
        }
        let tests = results
            .tests
            .iter()
            .map(|t| {
                [
                    text(t.code.expose()),
                    text(t.value.expose()),
                    text(t.flags.expose()),
                ]
            })
            .collect();
        Self {
            header: fields,
            tests,
        }
    }
}

impl Model for AuModel {
    fn syntax(&self) -> Syntax {
        Syntax::Au
    }

    fn get(&self, path: &Path) -> Option<Cow<'_, str>> {
        let FieldRef::Name(name) = path.field() else {
            return None;
        };
        if !path.is_absolute_in(Syntax::Au) {
            return None;
        }
        if path.record().is_some() {
            let test = self.tests.get(path.occurrence())?;
            let column = TEST_FIELDS.iter().position(|f| *f == name)?;
            return Some(Cow::Borrowed(test[column].as_str()));
        }
        self.header
            .get(name.as_str())
            .map(|v| Cow::Borrowed(v.as_str()))
    }

    fn set(&mut self, _path: &Path, _value: &str) -> Result<(), ModelError> {
        Err(ModelError::ReadOnly)
    }

    fn count(&self, record: &str) -> usize {
        if record == "test" {
            self.tests.len()
        } else {
            0
        }
    }

    fn create(&mut self, _record: &str) -> Result<usize, ModelError> {
        Err(ModelError::ReadOnly)
    }
}

#[cfg(test)]
mod tests {
    use packie_au::{Demographics, Redacted, ResultTest, SampleHeader};

    use super::*;

    fn bytes(text: &str) -> Redacted<Vec<u8>> {
        Redacted::new(text.as_bytes().to_vec())
    }

    fn sample() -> AuModel {
        let empty = || bytes("");
        AuModel::from_results(&SampleResults {
            kind: ResultKind::Normal,
            header: SampleHeader {
                unit_no: Some(bytes("01")),
                rack: bytes("0001"),
                cup: bytes("02"),
                sample_type: bytes(" "),
                sample_no: bytes("0007"),
                sample_id: bytes("  SYN0042"),
                reserved: bytes("    "),
                demographics: Some(Demographics {
                    sex: bytes("M"),
                    age: bytes("042"),
                    month: bytes("  "),
                    information: [bytes("ward 3"), empty(), empty(), empty(), empty(), empty()],
                }),
            },
            tests: vec![
                ResultTest {
                    code: bytes("01"),
                    value: bytes("  5.50"),
                    flags: bytes("  "),
                },
                ResultTest {
                    code: bytes("12"),
                    value: bytes(" 99.90"),
                    flags: bytes("H "),
                },
            ],
        })
    }

    fn p(text: &str) -> Path {
        Path::parse(Syntax::Au, text).expect("valid path")
    }

    #[test]
    fn reads_trimmed_header_fields_and_tests() {
        let m = sample();
        assert_eq!(m.get(&p("kind")).as_deref(), Some("normal"));
        assert_eq!(m.get(&p("sample_id")).as_deref(), Some("SYN0042"));
        assert_eq!(m.get(&p("rack")).as_deref(), Some("0001"));
        assert_eq!(m.get(&p("age")).as_deref(), Some("042"));
        assert_eq!(m.get(&p("month")).as_deref(), Some(""));
        assert_eq!(m.get(&p("info1")).as_deref(), Some("ward 3"));
        assert_eq!(m.get(&p("test.code")).as_deref(), Some("01"));
        assert_eq!(m.get(&p("test[0].value")).as_deref(), Some("5.50"));
        assert_eq!(m.get(&p("test[1].flags")).as_deref(), Some("H"));
        assert_eq!(m.get(&p("test[2].code")), None);
        assert_eq!(m.get(&p("nothing")), None);
        assert_eq!(m.count("test"), 2);
        assert_eq!(m.count("col"), 0);
    }

    #[test]
    fn absent_profile_fields_read_nothing() {
        let mut results = sample();
        results.header.remove("unit_no");
        assert_eq!(results.get(&p("unit_no")), None);
    }

    #[test]
    fn the_model_is_read_only() {
        let mut m = sample();
        assert_eq!(m.set(&p("sample_id"), "x"), Err(ModelError::ReadOnly));
        assert_eq!(m.create("test"), Err(ModelError::ReadOnly));
    }
}
