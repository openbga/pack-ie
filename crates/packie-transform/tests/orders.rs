//! Order download shapes: `for_each` mode `repeat` (several HL7 OBR tests into the
//! repetitions of one ASTM O-5) and the `require` input preconditions that refuse messages
//! the mapping cannot attribute.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use packie_transform::{AstmModel, Hl7Model, Mapping, Reason, Schema, TransformError};
use serde_json::{Value, json};

/// One order line: (order control, specimen id, test code).
type Line<'a> = (&'a str, &'a str, &'a str);

/// A synthetic ORM^O01 for one patient with one ORC/OBR pair per line, plus `extra`
/// segments after the PID.
fn orm(lines: &[Line<'_>], extra: &str) -> Hl7Model {
    let mut text = String::from(
        "MSH|^~\\&|HIS|FAC|LAB|INST|20261012120000||ORM^O01|M1|P|2.5.1\rPID|1||SYN-1||DOE^JANE\r",
    );
    text.push_str(extra);
    for (i, (control, specimen, test)) in lines.iter().enumerate() {
        text.push_str(&format!(
            "ORC|{control}|PLAC-{i}\rOBR|{}|PLAC-{i}|{specimen}|{test}^Test {test}\r",
            i + 1
        ));
    }
    Hl7Model::parse(text.as_bytes()).expect("synthetic ORM parses")
}

/// The order-download mapping: patient id, one O record, every OBR test as a repetition
/// of O-5 component 4, guarded by the preconditions of a one-specimen download.
fn download() -> Value {
    json!({
        "require": [
            {"count": "PID", "max": 1},
            {"count": "SPM", "max": 1},
            {"count": "OBR", "min": 1, "max": 18},
            {"same": "OBR.3.1"},
            {"same": "ORC.1"}
        ],
        "rules": [
            {"op": "copy", "from": "PID.3.1", "to": "P.3", "required": true},
            {"op": "constant", "value": "1", "to": "P.2"},
            {"op": "constant", "value": "1", "to": "O.2"},
            {"op": "copy", "from": "OBR.3.1", "to": "O.3", "required": true},
            {"op": "for_each", "over": "OBR", "into": "O.5", "mode": "repeat", "rules": [
                {"op": "copy", "from": "$.4.1", "to": "$.5.4", "required": true}
            ]},
            {"op": "constant", "value": "N", "to": "O.12"}
        ]
    })
}

fn run(source: &Hl7Model) -> Result<String, TransformError> {
    let mapping = Mapping::compile(&download(), &Schema::hl7(), &Schema::astm(), "m")
        .unwrap_or_else(|problems| panic!("expected a valid mapping: {problems:?}"));
    let mut target = AstmModel::new();
    mapping.apply(source, &mut target)?;
    Ok(String::from_utf8(target.encode())
        .expect("ascii")
        .replace('\r', "\n"))
}

fn tests(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("T{i:02}")).collect()
}

fn lines<'a>(codes: &'a [String], specimen: &'a str) -> Vec<Line<'a>> {
    codes
        .iter()
        .map(|code| ("NW", specimen, code.as_str()))
        .collect()
}

#[test]
fn eighteen_tests_become_repetitions_of_one_order_record() {
    let codes = tests(18);
    let out = run(&orm(&lines(&codes, "SPEC-1"), "")).expect("one specimen, 18 tests");
    let universal: Vec<String> = codes.iter().map(|code| format!("^^^{code}")).collect();
    assert_eq!(
        out,
        format!(
            "H|\\^&\nP|1|SYN-1\nO|1|SPEC-1||{}|||||||N\nL|1|N\n",
            universal.join("\\")
        )
    );
    // A single test is one repetition.
    let one = run(&orm(&[("NW", "SPEC-1", "GLU")], "")).expect("one test");
    assert_eq!(
        one,
        "H|\\^&\nP|1|SYN-1\nO|1|SPEC-1||^^^GLU|||||||N\nL|1|N\n"
    );
}

/// The `require` failure: rule path, path and reason, with no message value in the text.
fn refused(source: &Hl7Model) -> TransformError {
    let error = run(source).expect_err("refused");
    let text = error.to_string();
    for value in ["SYN-1", "SPEC", "DOE", "PLAC"] {
        assert!(!text.contains(value), "{text} shows a value");
    }
    error
}

#[test]
fn preconditions_refuse_what_one_order_record_cannot_carry() {
    let error = refused(&orm(&lines(&tests(19), "SPEC-1"), ""));
    assert_eq!(
        error.to_string(),
        "rule require[2] (require) at path OBR: the message has 19 occurrences; required 1 to 18"
    );
    assert_eq!(
        error.reason,
        Reason::Count {
            found: 19,
            min: Some(1),
            max: Some(18)
        }
    );
    let error = refused(&orm(&[("NW", "SPEC-1", "GLU"), ("NW", "SPEC-2", "NA")], ""));
    assert_eq!(
        error.to_string(),
        "rule require[3] (require) at path OBR.3.1: the occurrences hold different values"
    );
    let error = refused(&orm(&[("NW", "SPEC-1", "GLU"), ("CA", "SPEC-1", "NA")], ""));
    assert_eq!(
        (error.rule_path.as_str(), error.path.as_str()),
        ("require[4]", "ORC.1")
    );
    assert_eq!(error.reason, Reason::Differ);
    let error = refused(&orm(&[("NW", "SPEC-1", "GLU")], "PID|2||SYN-2\r"));
    assert_eq!(
        error.to_string(),
        "rule require[0] (require) at path PID: the message has 2 occurrences; required at most 1"
    );
    let error = refused(&orm(&[("NW", "SPEC-1", "GLU")], "SPM|1|S1\rSPM|2|S2\r"));
    assert_eq!(
        (error.rule_path.as_str(), error.path.as_str()),
        ("require[1]", "SPM")
    );
    let error = refused(&orm(&[], ""));
    assert_eq!(
        error.reason.to_string(),
        "the message has 0 occurrences; required 1 to 18"
    );
}

#[test]
fn repeat_mode_writes_repetition_i_and_leaves_skipped_ones_empty() {
    let mapping = Mapping::compile(
        &json!({"rules": [{"op": "for_each", "over": "OBR", "into": "O.5", "mode": "repeat", "rules": [
            {"op": "when", "condition": {"path": "$.4.1", "op": "not_equals", "value": "SKIP"}, "then": [
                {"op": "copy", "from": "$.4.1", "to": "$.5.4"},
                {"op": "sequence", "to": "$.5.1"}]},
            {"op": "copy", "from": "$.3.1", "to": "O.3"}
        ]}]}),
        &Schema::hl7(),
        &Schema::astm(),
        "m",
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    let source = orm(
        &[("NW", "S", "GLU"), ("NW", "S", "SKIP"), ("NW", "S", "NA")],
        "",
    );
    let mut target = AstmModel::new();
    mapping.apply(&source, &mut target).expect("runs");
    assert_eq!(
        String::from_utf8(target.encode()).expect("ascii"),
        "H|\\^&\rO||S||1^^^GLU\\\\3^^^NA\rL|1|N\r"
    );
}

fn problems(mapping: &Value, target: &Schema) -> Vec<String> {
    Mapping::compile(mapping, &Schema::hl7(), target, "m")
        .expect_err("invalid")
        .into_iter()
        .map(|p| p.to_string())
        .collect()
}

#[test]
fn validation_reports_require_and_repeat_problems() {
    let astm = Schema::astm();
    let mapping = json!({
        "require": [
            {"count": "obr", "max": 1},
            {"count": "OBR"},
            {"count": "OBR", "min": 3, "max": 2},
            {"count": "OBR", "max": -1},
            {"same": "$.3"},
            {"same": "OBR[1].3"},
            {"same": "OBR.3", "max": 1},
            {"exists": "PID.3"}
        ],
        "rules": [
            {"op": "for_each", "over": "OBR", "into": "O.5[1]", "mode": "repeat", "rules": []},
            {"op": "for_each", "over": "OBR", "into": "O.5", "mode": "repeat", "rules": [
                {"op": "copy", "from": "$.4", "to": "$.4.4"},
                {"op": "copy", "from": "$.4", "to": "$.5[1].4"},
                {"op": "copy", "from": "$.4", "to": "$.5.4"}
            ]},
            {"op": "for_each", "over": "OBR", "into": "O.5", "mode": "sideways", "rules": []}
        ]
    });
    assert_eq!(
        problems(&mapping, &astm),
        [
            "m.require[0].count: expected a three-character segment name such as OBX",
            "m.require[1]: needs `min`, `max` or both",
            "m.require[2].min: must not be greater than `max`",
            "m.require[3].max: must be an integer from 0 to 65535",
            "m.require[4].same: `$.` paths are only valid inside for_each",
            "m.require[5].same: write the path without an occurrence; every occurrence is compared",
            "m.require[6].max: unknown field",
            "m.require[7]: a condition is {\"count\": …} or {\"same\": …}",
            "m.rules[0].into: with mode `repeat`, into is a field path such as O.5",
            "m.rules[1].rules[0].to: inside mode `repeat`, a `$.` to is a path of field 5 without a repetition, for example $.5.4",
            "m.rules[1].rules[1].to: inside mode `repeat`, a `$.` to is a path of field 5 without a repetition, for example $.5.4",
            "m.rules[2].mode: must be one of create, update, repeat",
        ]
    );
    let delimited = Schema::delimited(["test"]);
    let repeat = json!({"rules": [
        {"op": "for_each", "over": "OBR", "into": "col.test", "mode": "repeat", "rules": []}]});
    assert_eq!(
        problems(&repeat, &delimited),
        ["m.rules[0].mode: mode `repeat` needs an HL7 or ASTM target"]
    );
    let not_a_list = json!({"require": {"count": "OBR", "max": 1}, "rules": []});
    assert_eq!(
        problems(&not_a_list, &astm),
        ["m.require: must be an array of conditions"]
    );
}
