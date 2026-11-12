//! End to end: an ASTM result message becomes an HL7 ORU^R01, the way a message channel
//! with an ASTM source and an HL7 destination runs it. The expected output is byte-exact.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use packie_transform::{
    AstmModel, Filters, Hl7Model, Mapping, Model, Path, Reason, Schema, Syntax,
};
use serde_json::{Value, json};

const RESULTS: &[u8] = b"H|\\^&|||CHEM-1^1.0|||||||P|LIS2-A2|20261012120000\r\
P|1|PRAC-1|LAB-1||DOE^JANE\r\
O|1|SYN-0001||^^^PANEL|R|20261012113000\r\
R|1|^^^GLU|5.5|mmol/L|3.9-6.1|N||F||OP||20261012115959\r\
R|2|^^^K|6.1|mmol/L|3.5-5.1|H||F||OP||20261012120105\r\
R|3|^^^NA|139|mmol/L|135-145|||F||OP||20261012120201\r\
L|1|N\r";

const EXPECTED_ORU: &str = "MSH|^~\\&|PACKIE|LAB|LIS|HOSP|20261012120500||ORU^R01|CTRL-0001|P|2.5.1\r\
PID|||PRAC-1||DOE^JANE\r\
OBR|||SYN-0001|PANEL|||20261012153000\r\
OBX|1|NM|2345-7^Glucose^LN||5.5|mmol/L|3.9-6.1|N|||F|||20261012155959\r\
OBX|2|NM|2823-3^Potassium^LN||6.1|mmol/L|3.5-5.1|H|||F|||20261012160105\r\
OBX|3|NM|2951-2^Sodium^LN||139|mmol/L|135-145||||F|||20261012160201\r";

fn mapping_json() -> Value {
    json!({
        "tables": {
            "loinc": {
                "GLU": "2345-7^Glucose^LN",
                "K": "2823-3^Potassium^LN",
                "NA": "2951-2^Sodium^LN"
            },
            "flags": { "H": "H", "L": "L", "N": "N" }
        },
        "rules": [
            { "op": "constant", "value": "ORU^R01", "to": "MSH.9" },
            { "op": "copy", "from": "P.3", "to": "PID.3", "required": true },
            { "op": "copy", "from": "P.6", "to": "PID.5" },
            { "op": "copy", "from": "O.3", "to": "OBR.3" },
            { "op": "copy", "from": "O.5.4", "to": "OBR.4.1" },
            { "op": "datetime", "from": "O.7", "to": "OBR.7",
              "input": "yyyyMMddHHmmss", "output": "yyyyMMddHHmmss",
              "from_timezone": "America/New_York", "to_timezone": "UTC" },
            { "op": "for_each", "over": "R", "into": "OBX", "rules": [
                { "op": "sequence", "to": "$.1" },
                { "op": "constant", "value": "NM", "to": "$.2" },
                { "op": "lookup", "from": "$.3.4", "table": "loinc", "to": "$.3" },
                { "op": "copy", "from": "$.4", "to": "$.5" },
                { "op": "copy", "from": "$.5", "to": "$.6" },
                { "op": "copy", "from": "$.6", "to": "$.7" },
                { "op": "lookup", "from": "$.7", "table": "flags", "to": "$.8", "on_missing": "skip" },
                { "op": "copy", "from": "$.9", "to": "$.11", "default": "F" },
                { "op": "datetime", "from": "$.13", "to": "$.14",
                  "input": "yyyyMMddHHmmss", "output": "yyyyMMddHHmmss",
                  "from_timezone": "America/New_York", "to_timezone": "UTC" }
            ] }
        ]
    })
}

fn compile(mapping: &Value) -> Mapping {
    Mapping::compile(mapping, &Schema::astm(), &Schema::hl7(), "mapping")
        .unwrap_or_else(|problems| panic!("mapping should be valid: {problems:?}"))
}

/// The destination's starting message: the header fields the pipeline generates from the
/// connector settings, a control id and the creation time. MSH-9 comes from the mapping.
fn skeleton() -> Hl7Model {
    let mut message = Hl7Model::new();
    for (path, value) in [
        ("MSH.3", "PACKIE"),
        ("MSH.4", "LAB"),
        ("MSH.5", "LIS"),
        ("MSH.6", "HOSP"),
        ("MSH.7", "20261012120500"),
        ("MSH.10", "CTRL-0001"),
        ("MSH.11", "P"),
        ("MSH.12", "2.5.1"),
    ] {
        let path = Path::parse(Syntax::Hl7, path).expect("valid path");
        message.set(&path, value).expect("generated header field");
    }
    message
}

fn source() -> AstmModel {
    AstmModel::parse(RESULTS).expect("synthetic ASTM message parses")
}

#[test]
fn the_fixture_is_a_valid_astm_result_message() {
    let records = packie_astm::parse_message_records(RESULTS).expect("records parse");
    let message =
        packie_astm::assemble_message(records).expect("hierarchy and sequences are valid");
    assert_eq!(message.patients()[0].orders()[0].results().len(), 3);
}

#[test]
fn astm_results_become_a_byte_stable_oru() {
    let mapping = compile(&mapping_json());
    let source = source();
    let mut first = skeleton();
    mapping.apply(&source, &mut first).expect("mapping applies");
    let mut second = skeleton();
    mapping
        .apply(&source, &mut second)
        .expect("mapping applies");
    assert_eq!(
        String::from_utf8(first.encode()).expect("ascii"),
        EXPECTED_ORU
    );
    assert_eq!(first.encode(), second.encode());
    // the source message is never modified
    assert_eq!(source.encode(), RESULTS.to_vec());
}

#[test]
fn the_oru_is_a_valid_hl7_message_with_numbered_obx_segments() {
    let mapping = compile(&mapping_json());
    let mut target = skeleton();
    mapping
        .apply(&source(), &mut target)
        .expect("mapping applies");
    let parsed =
        packie_hl7::Message::parse(&target.encode()).expect("the codec accepts the output");
    assert_eq!(parsed.get("MSH-9.2"), Some("R01"));
    for (n, (code, value)) in [("2345-7", "5.5"), ("2823-3", "6.1"), ("2951-2", "139")]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            parsed.get(&format!("OBX({})-1", n + 1)),
            Some((n + 1).to_string().as_str())
        );
        assert_eq!(parsed.get(&format!("OBX({})-3.1", n + 1)), Some(code));
        assert_eq!(parsed.get(&format!("OBX({})-5", n + 1)), Some(value));
    }
    assert!(parsed.get("OBX(4)-1").is_none());
}

#[test]
fn an_unknown_test_code_fails_the_mapping_without_naming_the_value() {
    let mut mapping = mapping_json();
    mapping["tables"]["loinc"]
        .as_object_mut()
        .expect("table")
        .remove("K");
    let mut target = skeleton();
    let error = compile(&mapping)
        .apply(&source(), &mut target)
        .expect_err("potassium has no code");
    assert_eq!(error.rule, 6);
    assert_eq!(error.rule_path, "rules[6].rules[2]");
    assert_eq!(error.op, "lookup");
    assert_eq!(error.path, "$.3.4");
    assert_eq!(
        error.reason,
        Reason::LookupMissing {
            table: "loinc".into()
        }
    );
    assert_eq!(
        error.to_string(),
        "rule rules[6].rules[2] (lookup) at path $.3.4: no entry in code table `loinc`"
    );
}

#[test]
fn a_missing_required_patient_id_fails_the_mapping() {
    let source =
        AstmModel::parse(b"H|\\^&|||X\rP|1\rO|1|S1\rR|1|^^^GLU|5\rL|1|N\r").expect("parses");
    let error = compile(&mapping_json())
        .apply(&source, &mut skeleton())
        .expect_err("P.3 is empty");
    assert_eq!(
        (error.rule, error.op, error.path.as_str()),
        (1, "copy", "P.3")
    );
    assert_eq!(error.reason, Reason::RequiredMissing);
}

#[test]
fn filters_select_results_before_the_mapping_runs() {
    let schema = Schema::astm();
    let abnormal = json!({"path": "R.7", "op": "in", "value": ["H", "L"]});
    let from_chemistry =
        json!({"path": "H.5.1", "op": "starts_with", "value": "chem", "ignore_case": true});
    let wanted = Filters::compile(
        std::slice::from_ref(&from_chemistry),
        &schema,
        "channels[0].filters",
    )
    .expect("valid");
    assert!(wanted.passes(&source()));
    // R.7 is read from the first R record only; the first result is normal
    let only_abnormal =
        Filters::compile(&[from_chemistry, abnormal], &schema, "channels[0].filters")
            .expect("valid");
    assert!(!only_abnormal.passes(&source()));
    let any_result_abnormal = json!({"any": [
        {"path": "R[0].7", "op": "in", "value": ["H", "L"]},
        {"path": "R[1].7", "op": "in", "value": ["H", "L"]},
        {"path": "R[2].7", "op": "in", "value": ["H", "L"]}]});
    let filter = Filters::compile(&[any_result_abnormal], &schema, "f").expect("valid");
    assert!(filter.passes(&source()));
}
