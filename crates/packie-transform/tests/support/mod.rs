//! Fixtures shared by the integration tests. Each test crate uses a subset.
#![allow(dead_code, clippy::expect_used)]

use packie_au::{Redacted, ResultKind, ResultTest, SampleHeader, SampleResults};
use packie_transform::Hl7Model;

/// A synthetic HL7 result message: one patient and three observations (the second is
/// non-numeric, the third is potassium).
pub(crate) const ORU: &[u8] = b"MSH|^~\\&|APP|FAC|LIS|LAB|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||SYN-1^^^HOSP||DOE^JANE\rOBX|1|NM|GLU^Glucose||5.50|mmol/L|3.9-6.1|H\rOBX|2|ST|NOTE||<5|||\rOBX|3|NM|K^Potassium||4.1|mmol/L|3.5-5.1|N\r";

/// [`ORU`] parsed.
pub(crate) fn oru() -> Hl7Model {
    Hl7Model::parse(ORU).expect("synthetic message parses")
}

/// A synthetic completed AU sample with two result items and no optional header fields.
pub(crate) fn au_sample() -> SampleResults {
    let bytes = |text: &str| Redacted::new(text.as_bytes().to_vec());
    SampleResults {
        kind: ResultKind::Normal,
        header: SampleHeader {
            unit_no: None,
            rack: bytes("0001"),
            cup: bytes("02"),
            sample_type: bytes(" "),
            sample_no: bytes("0007"),
            sample_id: bytes("  SYN0042"),
            reserved: bytes("    "),
            demographics: None,
        },
        tests: vec![
            ResultTest {
                code: bytes("01"),
                value: bytes("  5.50"),
                flags: bytes("  "),
            },
            ResultTest {
                code: bytes("12"),
                value: bytes("123.40"),
                flags: bytes("H "),
            },
        ],
    }
}
