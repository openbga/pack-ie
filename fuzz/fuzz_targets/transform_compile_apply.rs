//! Filters and mappings compiled from JSON in the input, then run on seed messages. Byte 0
//! picks the source protocol (HL7, ASTM, delimited rows, AU), byte 1 the target protocol and
//! whether the document is a filter or a mapping; the rest is the JSON document.
#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use packie_au::{ResultKind, ResultTest, SampleHeader, SampleResults};
use packie_transform::{
    AstmModel, AuModel, Filter, Filters, Hl7Model, Mapping, Model, RecordModel, Schema,
};

const ORU: &[u8] = b"MSH|^~\\&|APP|FAC|LIS|LAB|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||SYN-1^^^HOSP||DOE^JANE\rOBR|1||S1|PANEL\rOBX|1|NM|GLU^Glucose||5.50|mmol/L|3.9-6.1|H\rOBX|2|ST|NOTE||<5|||\rOBX|3|NM|K^Potassium||4.1|mmol/L|3.5-5.1|N\r";
const ASTM: &[u8] = b"H|\\^&|||CHEM-1^1.0|||||||P|LIS2-A2|20261012120000\rP|1|PRAC-1|LAB-1||DOE^JANE\rO|1|SYN-0001||^^^PANEL|R|20261012113000\rR|1|^^^GLU|5.5|mmol/L|3.9-6.1|N||F||OP||20261012115959\rR|2|^^^K|6.1|mmol/L|3.5-5.1|H||F||OP||20261012120105\rL|1|N\r";
const COLUMNS: [&str; 4] = ["sample_id", "test", "result", "unit"];

fn redacted(text: &[u8]) -> packie_au::Redacted<Vec<u8>> {
    packie_au::Redacted::new(text.to_vec())
}

fn au_sample() -> SampleResults {
    SampleResults {
        kind: ResultKind::Normal,
        header: SampleHeader {
            unit_no: Some(redacted(b"01")),
            rack: redacted(b"0001"),
            cup: redacted(b"01"),
            sample_type: redacted(b"U"),
            sample_no: redacted(b"0001"),
            sample_id: redacted(b"SYN00001"),
            reserved: redacted(b""),
            demographics: None,
        },
        tests: vec![
            ResultTest {
                code: redacted(b"01"),
                value: redacted(b"012.34"),
                flags: redacted(b"  "),
            },
            ResultTest {
                code: redacted(b"12"),
                value: redacted(b"004.10"),
                flags: redacted(b"H "),
            },
        ],
    }
}

fn rows() -> RecordModel {
    RecordModel::from_rows([
        [
            ("sample_id", "S1"),
            ("test", "GLU"),
            ("result", "5.5"),
            ("unit", "mmol/L"),
        ],
        [
            ("sample_id", "S1"),
            ("test", "K"),
            ("result", "4.1"),
            ("unit", "mmol/L"),
        ],
    ])
}

fn source(kind: u8) -> (Box<dyn Model>, Schema) {
    match kind % 4 {
        0 => (
            Box::new(Hl7Model::parse(ORU).expect("seed parses")),
            Schema::hl7(),
        ),
        1 => (
            Box::new(AstmModel::parse(ASTM).expect("seed parses")),
            Schema::astm(),
        ),
        2 => (Box::new(rows()), Schema::delimited(COLUMNS)),
        _ => (Box::new(AuModel::from_results(&au_sample())), Schema::au()),
    }
}

/// A model to write into, kept concrete so the output can be encoded afterwards.
enum Target {
    Hl7(Hl7Model),
    Astm(AstmModel),
    Rows(RecordModel),
}

impl Target {
    fn new(kind: u8) -> (Self, Schema) {
        match kind % 3 {
            0 => (Self::Hl7(Hl7Model::new()), Schema::hl7()),
            1 => (Self::Astm(AstmModel::new()), Schema::astm()),
            _ => (Self::Rows(RecordModel::new()), Schema::delimited(COLUMNS)),
        }
    }

    fn model(&mut self) -> &mut dyn Model {
        match self {
            Self::Hl7(model) => model,
            Self::Astm(model) => model,
            Self::Rows(model) => model,
        }
    }

    /// Encodes what was written; an HL7 result is parsed again (it may legitimately be
    /// rejected, for example for missing header fields).
    fn finish(&self) {
        match self {
            Self::Hl7(model) => {
                let _ = Hl7Model::parse(&model.encode());
            }
            Self::Astm(model) => {
                let _ = AstmModel::parse(&model.encode());
            }
            Self::Rows(model) => {
                let _ = model.count("col");
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(([source_kind, control], json)) = data.split_first_chunk::<2>() else {
        return;
    };
    let Ok(document) = serde_json::from_slice::<Value>(json) else {
        return;
    };
    let (model, schema) = source(*source_kind);
    if control & 0x80 != 0 {
        if let Value::Array(values) = &document {
            if let Ok(filters) = Filters::compile(values, &schema, "filters") {
                let _ = filters.passes(model.as_ref());
            }
        } else if let Ok(filter) = Filter::compile(&document, &schema, "filter") {
            let _ = filter.matches(model.as_ref());
        }
        return;
    }
    let (mut output, target_schema) = Target::new(*control);
    let Ok(mapping) = Mapping::compile(&document, &schema, &target_schema, "mapping") else {
        return;
    };
    let _ = (mapping.len(), mapping.is_empty(), mapping.tables().len());
    let _ = mapping.apply(model.as_ref(), output.model());
    output.finish();
    // Applying again onto what the first run left behind: rules that overwrite or append must
    // not trip over existing records.
    let _ = mapping.apply(model.as_ref(), output.model());
    output.finish();
});
