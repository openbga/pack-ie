//! Mapping rules, one behavior each, plus validation and error attribution.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod support;

use packie_transform::{
    AstmModel, AuModel, Hl7Model, Mapping, Model, ModelError, Reason, RecordModel, Schema,
    TransformError,
};
use proptest::prelude::*;
use serde_json::{Value, json};
use support::{ORU, au_sample, oru};

fn hl7_mapping(mapping: &Value) -> Mapping {
    Mapping::compile(mapping, &Schema::hl7(), &Schema::hl7(), "mapping")
        .unwrap_or_else(|problems| panic!("expected a valid mapping: {problems:?}"))
}

/// Runs `mapping` over the sample message into a fresh HL7 message; the encoded output
/// with `\r` shown as newline.
fn run(mapping: &Value) -> Result<String, TransformError> {
    let mut target = Hl7Model::new();
    hl7_mapping(mapping).apply(&oru(), &mut target)?;
    Ok(String::from_utf8(target.encode())
        .expect("ascii")
        .replace('\r', "\n"))
}

fn run_ok(mapping: &Value) -> String {
    run(mapping).unwrap_or_else(|error| panic!("mapping failed: {error}"))
}

fn rules(rules: &Value) -> Value {
    json!({ "rules": rules })
}

#[test]
fn copy_moves_text_between_paths() {
    let out = run_ok(&rules(&json!([
        {"op": "copy", "from": "PID.3.1", "to": "PID.4"},
        {"op": "copy", "from": "PID.5", "to": "PID.6"},
        {"op": "copy", "from": "MSH.9", "to": "ZZZ.1"},
        {"op": "copy", "from": "OBX[2].5", "to": "ZZZ.2.1.1"}
    ])));
    assert_eq!(out, "MSH|^~\\&\nPID||||SYN-1||DOE^JANE\nZZZ|ORU^R01|4.1\n");
}

#[test]
fn copy_uses_default_for_empty_or_missing_and_writes_nothing_otherwise() {
    let out = run_ok(&rules(&json!([
        {"op": "copy", "from": "PID.4", "to": "PID.4", "default": "UNKNOWN"},
        {"op": "copy", "from": "PID.99", "to": "PID.5", "default": 7},
        {"op": "copy", "from": "PID.98", "to": "PID.6"},
        {"op": "copy", "from": "PID.3.1", "to": "PID.7", "default": "unused"}
    ])));
    assert_eq!(out, "MSH|^~\\&\nPID||||UNKNOWN|7||SYN-1\n");
}

#[test]
fn required_copy_fails_naming_rule_and_path_but_not_the_value() {
    let error = run(&rules(&json!([
        {"op": "constant", "value": "x", "to": "PID.1"},
        {"op": "copy", "from": "PID.4", "to": "PID.2", "required": true}
    ])))
    .expect_err("PID.4 is empty");
    assert_eq!(error.rule, 1);
    assert_eq!(error.rule_path, "rules[1]");
    assert_eq!(error.op, "copy");
    assert_eq!(error.path, "PID.4");
    assert_eq!(error.reason, Reason::RequiredMissing);
    assert_eq!(
        error.to_string(),
        "rule rules[1] (copy) at path PID.4: required value is missing"
    );
    // a default satisfies `required`
    let out = run_ok(&rules(&json!([
        {"op": "copy", "from": "PID.4", "to": "PID.2", "required": true, "default": "D"}
    ])));
    assert!(out.contains("PID||D"));
}

#[test]
fn constant_writes_text_even_when_empty() {
    let out = run_ok(&rules(&json!([
        {"op": "constant", "value": "ORU^R01", "to": "MSH.9"},
        {"op": "constant", "value": 42, "to": "PID.1"},
        {"op": "constant", "value": "", "to": "PID.2"}
    ])));
    assert_eq!(out, "MSH|^~\\&|||||||ORU^R01\nPID|42|\n");
}

fn lookup(extra: &Value) -> Value {
    let mut rule = json!({"op": "lookup", "from": "OBX.3.1", "table": "codes", "to": "OBX.3"});
    if let (Some(rule), Some(extra)) = (rule.as_object_mut(), extra.as_object()) {
        rule.extend(extra.clone());
    }
    json!({
        "tables": {"codes": {"GLU": "2345-7^Glucose^LN", "k": "2823-3^Potassium^LN"}},
        "rules": [rule]
    })
}

#[test]
fn lookup_maps_codes_through_a_table() {
    assert_eq!(
        run_ok(&lookup(&json!({}))),
        "MSH|^~\\&\nOBX|||2345-7^Glucose^LN\n"
    );
}

#[test]
fn lookup_on_missing_error_is_the_default() {
    let mapping = json!({
        "tables": {"codes": {"X": "y"}},
        "rules": [{"op": "lookup", "from": "OBX.3.1", "table": "codes", "to": "OBX.3"}]
    });
    let error = run(&mapping).expect_err("GLU is not in the table");
    assert_eq!(error.op, "lookup");
    assert_eq!(error.path, "OBX.3.1");
    assert_eq!(
        error.reason,
        Reason::LookupMissing {
            table: "codes".into()
        }
    );
    assert!(!error.to_string().contains("GLU"));
}

#[test]
fn lookup_on_missing_keep_default_and_skip() {
    let table = json!({"codes": {"X": "y"}});
    let mapping = |on_missing: Value, default: Option<&str>| {
        let mut rule = json!({"op": "lookup", "from": "OBX.3.1", "table": "codes", "to": "ZZZ.1", "on_missing": on_missing});
        if let Some(default) = default {
            rule["default"] = json!(default);
        }
        json!({"tables": table, "rules": [rule]})
    };
    assert_eq!(
        run_ok(&mapping(json!("keep"), None)),
        "MSH|^~\\&\nZZZ|GLU\n"
    );
    assert_eq!(
        run_ok(&mapping(json!("default"), Some("OTHER"))),
        "MSH|^~\\&\nZZZ|OTHER\n"
    );
    assert_eq!(run_ok(&mapping(json!("skip"), None)), "MSH|^~\\&\n");
}

#[test]
fn lookup_empty_source_counts_as_a_miss() {
    let table = json!({"codes": {"X": "y"}});
    let skip = json!({"tables": table, "rules": [
        {"op": "lookup", "from": "PID.4", "table": "codes", "to": "ZZZ.1", "on_missing": "skip"}]});
    assert_eq!(run_ok(&skip), "MSH|^~\\&\n");
    let keep = json!({"tables": table, "rules": [
        {"op": "lookup", "from": "PID.4", "table": "codes", "to": "ZZZ.1", "on_missing": "keep"}]});
    assert_eq!(run_ok(&keep), "MSH|^~\\&\n");
}

#[test]
fn lookup_matching_is_exact_unless_ignore_case_is_set() {
    // table key is `k`, message code is `K`
    let exact = json!({
        "tables": {"codes": {"k": "potassium"}},
        "rules": [{"op": "lookup", "from": "OBX[2].3.1", "table": "codes", "to": "ZZZ.1", "on_missing": "skip"}]
    });
    assert_eq!(run_ok(&exact), "MSH|^~\\&\n");
    let folded = json!({
        "tables": {"codes": {"k": "potassium"}},
        "rules": [{"op": "lookup", "from": "OBX[2].3.1", "table": "codes", "to": "ZZZ.1", "ignore_case": true}]
    });
    assert_eq!(run_ok(&folded), "MSH|^~\\&\nZZZ|potassium\n");
}

#[test]
fn concat_joins_paths_and_text_in_order() {
    let out = run_ok(&rules(&json!([
        {"op": "concat", "to": "ZZZ.1", "parts": [
            {"from": "PID.5.1"}, {"text": ", "}, {"from": "PID.5.2"}, {"text": 7}, {"from": "PID.99"}]},
        {"op": "concat", "to": "ZZZ.2", "parts": [{"from": "PID.98"}]}
    ])));
    assert_eq!(out, "MSH|^~\\&\nZZZ|DOE, JANE7\n");
}

#[test]
fn datetime_reformats_and_converts_time_zones() {
    let out = run_ok(&rules(&json!([
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.1", "input": "yyyyMMddHHmmss", "output": "yyyy-MM-dd'T'HH:mm:ss"},
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.2", "input": "yyyyMMddHHmmss", "output": "yyyyMMddHHmmss",
         "from_timezone": "UTC", "to_timezone": "America/New_York"},
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.3", "input": "yyyyMMddHHmmss", "output": "dd.MM.yy"},
        {"op": "datetime", "from": "PID.4", "to": "ZZZ.4", "input": "yyyyMMdd", "output": "yyyyMMdd"}
    ])));
    assert_eq!(
        out,
        "MSH|^~\\&\nZZZ|2026-10-12T12:00:00|20261012080000|12.10.26\n"
    );
}

#[test]
fn datetime_failures_are_transform_errors() {
    let bad_input = run(&rules(&json!([
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.1", "input": "yyyyMMdd", "output": "yyyyMMdd"}
    ])))
    .expect_err("the timestamp is longer than the format");
    assert_eq!(bad_input.reason, Reason::DatetimeInput);
    assert_eq!(bad_input.path, "MSH.7");

    let mismatch = run(&rules(&json!([
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.1", "input": "yyyyMMdd'T'HHmm", "output": "yyyyMMddHHmmss"}
    ])))
    .expect_err("never matches");
    assert_eq!(mismatch.reason, Reason::DatetimeInput);

    let needs_seconds = run(&rules(&json!([
        {"op": "datetime", "from": "ZZZ.9", "to": "ZZZ.1", "input": "yyyyMMdd", "output": "yyyyMMddHHmmss"}
    ])))
    .expect("an empty source writes nothing");
    assert!(!needs_seconds.contains("ZZZ"));

    let mut hl7 = Hl7Model::new();
    let source =
        Hl7Model::parse(b"MSH|^~\\&|A|B|C|D|20261012||ORU^R01|M1|P|2.5.1\r").expect("parses");
    let error = hl7_mapping(&rules(&json!([
        {"op": "datetime", "from": "MSH.7", "to": "ZZZ.1", "input": "yyyyMMdd", "output": "yyyyMMddHHmmss"}
    ])))
    .apply(&source, &mut hl7)
    .expect_err("the date has no time");
    assert_eq!(error.reason, Reason::DatetimeMissing { component: "hour" });
}

#[test]
fn when_runs_then_or_else_by_condition() {
    let rule = |value: &str| {
        json!({"op": "when",
            "condition": {"path": "MSH.9.2", "op": "equals", "value": value},
            "then": [{"op": "constant", "value": "yes", "to": "ZZZ.1"}],
            "else": [{"op": "constant", "value": "no", "to": "ZZZ.1"}]})
    };
    assert_eq!(
        run_ok(&rules(&json!([rule("R01")]))),
        "MSH|^~\\&\nZZZ|yes\n"
    );
    assert_eq!(run_ok(&rules(&json!([rule("R03")]))), "MSH|^~\\&\nZZZ|no\n");
    let no_else = json!({"op": "when",
        "condition": {"not": {"path": "PID.3", "op": "exists"}},
        "then": [{"op": "constant", "value": "yes", "to": "ZZZ.1"}]});
    assert_eq!(run_ok(&rules(&json!([no_else]))), "MSH|^~\\&\n");
}

#[test]
fn for_each_creates_one_output_occurrence_per_input_with_a_sequence() {
    let out = run_ok(&rules(&json!([
        {"op": "for_each", "over": "OBX", "into": "ZZZ", "rules": [
            {"op": "sequence", "to": "$.1"},
            {"op": "copy", "from": "$.3.1", "to": "$.2"},
            {"op": "copy", "from": "$.5", "to": "$.3", "default": "-"},
            {"op": "copy", "from": "PID.3.1", "to": "$.4"},
            {"op": "sequence", "to": "$.5", "start": 10}
        ]}
    ])));
    assert_eq!(
        out,
        "MSH|^~\\&\nZZZ|1|GLU|5.50|SYN-1|10\nZZZ|2|NOTE|<5|SYN-1|11\nZZZ|3|K|4.1|SYN-1|12\n"
    );
}

#[test]
fn for_each_over_no_occurrences_does_nothing_and_when_sees_the_current_occurrence() {
    let out = run_ok(&rules(&json!([
        {"op": "for_each", "over": "NTE", "into": "NTE", "rules": [
            {"op": "constant", "value": "never", "to": "$.1"}]},
        {"op": "for_each", "over": "OBX", "into": "ZZZ", "rules": [
            {"op": "when",
             "condition": {"path": "$.8", "op": "in", "value": ["H", "N"]},
             "then": [{"op": "copy", "from": "$.8", "to": "$.1"}],
             "else": [{"op": "constant", "value": "?", "to": "$.1"}]}]}
    ])));
    assert_eq!(out, "MSH|^~\\&\nZZZ|H\nZZZ|?\nZZZ|N\n");
}

#[test]
fn errors_name_the_nested_rule_and_the_top_level_index() {
    let mapping = json!({
        "tables": {"codes": {"GLU": "glucose"}},
        "rules": [
            {"op": "constant", "value": "x", "to": "ZZZ.1"},
            {"op": "for_each", "over": "OBX", "into": "ZZZ", "rules": [
                {"op": "sequence", "to": "$.1"},
                {"op": "when", "condition": {"path": "$.3.1", "op": "exists"}, "then": [
                    {"op": "lookup", "from": "$.3.1", "table": "codes", "to": "$.2"}]}]}
        ]
    });
    let error = run(&mapping).expect_err("NOTE is not in the table");
    assert_eq!(error.rule, 1);
    assert_eq!(error.rule_path, "rules[1].rules[1].then[0]");
    assert_eq!(error.op, "lookup");
    assert_eq!(error.path, "$.3.1");
    assert!(!error.to_string().contains("NOTE"));
}

#[test]
fn a_clone_of_the_source_can_be_the_starting_message() {
    let source = oru();
    let mut target = source.clone();
    let mapping = hl7_mapping(&json!({"rules": [
        {"op": "constant", "value": "REDACTED", "to": "PID.5"},
        {"op": "constant", "value": "ORU^R01", "to": "MSH.9"}
    ]}));
    mapping.apply(&source, &mut target).expect("applies");
    let out = String::from_utf8(target.encode()).expect("ascii");
    assert!(out.contains("PID|1||SYN-1^^^HOSP||REDACTED\r"));
    assert!(out.contains("OBX|1|NM|GLU^Glucose||5.50|mmol/L|3.9-6.1|H\r"));
    // the source is untouched
    assert_eq!(source.encode(), ORU.to_vec());
}

#[test]
fn hl7_to_delimited_rows() {
    let columns = ["sample_id", "test", "value", "units", "flag"];
    let schema = Schema::delimited(columns);
    let mapping = Mapping::compile(
        &json!({"rules": [
            {"op": "for_each", "over": "OBX", "into": "col", "rules": [
                {"op": "copy", "from": "PID.3.1", "to": "$.sample_id"},
                {"op": "copy", "from": "$.3.1", "to": "$.test"},
                {"op": "copy", "from": "$.5", "to": "$.value"},
                {"op": "copy", "from": "$.6", "to": "$.units"},
                {"op": "copy", "from": "$.8", "to": "$.flag", "default": "N"}]}]}),
        &Schema::hl7(),
        &schema,
        "mapping",
    )
    .expect("valid");
    let mut target = RecordModel::new();
    mapping.apply(&oru(), &mut target).expect("applies");
    let rows: Vec<Vec<&str>> = target
        .rows()
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|c| row.get(*c).map_or("", String::as_str))
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            vec!["SYN-1", "GLU", "5.50", "mmol/L", "H"],
            vec!["SYN-1", "NOTE", "<5", "", "N"],
            vec!["SYN-1", "K", "4.1", "mmol/L", "N"],
        ]
    );
}

#[test]
fn delimited_and_au_sources_feed_hl7() {
    let columns = Schema::delimited(["sample_id", "result"]);
    let rows = RecordModel::from_rows([
        [("sample_id", "S1"), ("result", "5.5")],
        [("sample_id", "S1"), ("result", "7.0")],
    ]);
    let mapping = Mapping::compile(
        &json!({"rules": [
            {"op": "copy", "from": "col.sample_id", "to": "OBR.3"},
            {"op": "for_each", "over": "col", "into": "OBX", "rules": [
                {"op": "sequence", "to": "$.1"},
                {"op": "copy", "from": "$.result", "to": "$.5"}]}]}),
        &columns,
        &Schema::hl7(),
        "mapping",
    )
    .expect("valid");
    let mut target = Hl7Model::new();
    mapping.apply(&rows, &mut target).expect("applies");
    assert_eq!(
        String::from_utf8(target.encode()).expect("ascii"),
        "MSH|^~\\&\rOBR|||S1\rOBX|1||||5.5\rOBX|2||||7.0\r"
    );

    let au = AuModel::from_results(&au_sample());
    let mapping = Mapping::compile(
        &json!({"tables": {"tests": {"01": "GLU", "12": "K"}}, "rules": [
            {"op": "copy", "from": "sample_id", "to": "OBR.3"},
            {"op": "for_each", "over": "test", "into": "OBX", "rules": [
                {"op": "sequence", "to": "$.1"},
                {"op": "lookup", "from": "$.code", "table": "tests", "to": "$.3"},
                {"op": "copy", "from": "$.value", "to": "$.5"},
                {"op": "copy", "from": "$.flags", "to": "$.8"}]}]}),
        &Schema::au(),
        &Schema::hl7(),
        "mapping",
    )
    .expect("valid");
    let mut target = Hl7Model::new();
    mapping.apply(&au, &mut target).expect("applies");
    assert_eq!(
        String::from_utf8(target.encode()).expect("ascii"),
        "MSH|^~\\&\rOBR|||SYN0042\rOBX|1||GLU||5.50\rOBX|2||K||123.40|||H\r"
    );
}

#[test]
fn hl7_to_astm_builds_records_between_header_and_terminator() {
    let mapping = Mapping::compile(
        &json!({"rules": [
            {"op": "copy", "from": "PID.3.1", "to": "P.3"},
            {"op": "copy", "from": "PID.5", "to": "P.6"},
            {"op": "for_each", "over": "OBX", "into": "R", "rules": [
                {"op": "sequence", "to": "$.2"},
                {"op": "copy", "from": "$.3.1", "to": "$.3.4"},
                {"op": "copy", "from": "$.5", "to": "$.4"},
                {"op": "copy", "from": "$.6", "to": "$.5"}]}]}),
        &Schema::hl7(),
        &Schema::astm(),
        "mapping",
    )
    .expect("valid");
    let mut target = AstmModel::new();
    mapping.apply(&oru(), &mut target).expect("applies");
    assert_eq!(
        String::from_utf8(target.encode()).expect("ascii"),
        "H|\\^&\rP||SYN-1|||DOE^JANE\rR|1|^^^GLU|5.50|mmol/L\rR|2|^^^NOTE|<5\rR|3|^^^K|4.1|mmol/L\rL|1|N\r"
    );
}

#[test]
fn read_only_targets_report_a_write_error_at_run_time() {
    // Compilation refuses AU targets, so drive a model that refuses at run time instead.
    struct Sealed(Hl7Model);
    impl Model for Sealed {
        fn syntax(&self) -> packie_transform::Syntax {
            self.0.syntax()
        }
        fn get(&self, path: &packie_transform::Path) -> Option<std::borrow::Cow<'_, str>> {
            self.0.get(path)
        }
        fn set(&mut self, _: &packie_transform::Path, _: &str) -> Result<(), ModelError> {
            Err(ModelError::ReadOnly)
        }
        fn count(&self, record: &str) -> usize {
            self.0.count(record)
        }
        fn create(&mut self, _: &str) -> Result<usize, ModelError> {
            Err(ModelError::ReadOnly)
        }
    }
    let mapping = hl7_mapping(&rules(
        &json!([{"op": "constant", "value": "x", "to": "PID.1"}]),
    ));
    let error = mapping
        .apply(&oru(), &mut Sealed(Hl7Model::new()))
        .expect_err("sealed");
    assert_eq!(error.reason, Reason::Write(ModelError::ReadOnly));
    assert_eq!(error.path, "PID.1");
    let mapping = hl7_mapping(&rules(&json!([
        {"op": "for_each", "over": "OBX", "into": "ZZZ", "rules": []}])));
    let error = mapping
        .apply(&oru(), &mut Sealed(Hl7Model::new()))
        .expect_err("sealed");
    assert_eq!(error.path, "ZZZ");
    assert_eq!(error.op, "for_each");
}

#[test]
fn compiled_mappings_expose_their_tables() {
    let mapping = hl7_mapping(&json!({
        "tables": {"flags": {"H": "High", "L": "Low"}},
        "rules": [{"op": "constant", "value": "x", "to": "ZZZ.1"}]
    }));
    assert_eq!(mapping.len(), 1);
    assert!(!mapping.is_empty());
    let table = mapping.tables().get("flags").expect("table");
    assert_eq!(table.lookup("H", false), Some("High"));
    assert_eq!(table.len(), 2);
}

#[test]
fn validation_reports_every_problem_with_its_path() {
    let mapping = json!({
        "tables": {"flags": {"H": "High", "x": 5}, "bad": 3, "ci": {"a": "1", "A": "2"}},
        "rules": [
            {"op": "frobnicate"},
            {"op": "copy", "from": "PID..3", "to": "PID.3"},
            {"op": "copy", "from": "PID.3"},
            {"op": "copy", "from": "col.x", "to": "PID.3", "extra": true},
            {"op": "copy", "from": "$.3", "to": "PID.3"},
            {"op": "copy", "from": "PID.3", "to": "MSH.2"},
            {"op": "lookup", "from": "PID.3", "table": "nope", "to": "PID.4"},
            {"op": "lookup", "from": "PID.3", "table": "flags", "to": "PID.4", "on_missing": "maybe"},
            {"op": "lookup", "from": "PID.3", "table": "flags", "to": "PID.4", "on_missing": "default"},
            {"op": "lookup", "from": "PID.3", "table": "flags", "to": "PID.4", "default": "x"},
            {"op": "lookup", "from": "PID.3", "table": "ci", "to": "PID.4", "ignore_case": true},
            {"op": "concat", "to": "PID.3", "parts": []},
            {"op": "concat", "to": "PID.3", "parts": [{"from": "PID.3", "text": "x"}, {}, 5]},
            {"op": "datetime", "from": "PID.3", "to": "PID.4", "input": "YYYY", "output": "yyyy-MM",
             "from_timezone": "Mars/Base", "to_timezone": 5},
            {"op": "when", "then": []},
            {"op": "when", "condition": {"path": "PID.3", "op": "nope"}, "then": [{"op": "frobnicate"}], "else": 1},
            {"op": "for_each", "over": "pid", "into": "MSH", "rules": [
                {"op": "for_each", "over": "OBX", "into": "OBX", "rules": []},
                {"op": "copy", "from": "$.", "to": "$.1"}]},
            {"op": "for_each", "over": "OBX", "into": "OBX"},
            {"op": "sequence", "to": "PID.1", "start": -1},
            {"op": "sequence", "to": "$.1"},
            {"op": "constant", "to": "PID.1"},
            {"to": "PID.1"},
            "text"
        ]
    });
    let problems =
        Mapping::compile(&mapping, &Schema::hl7(), &Schema::hl7(), "mapping").expect_err("invalid");
    let text: Vec<String> = problems.iter().map(ToString::to_string).collect();
    insta::assert_snapshot!(text.join("\n"));
}

#[test]
fn validation_checks_names_against_each_protocols_model() {
    let columns = Schema::delimited(["sample_id"]);
    let cases: [(&Schema, &Schema, Value); 6] = [
        (
            &columns,
            &Schema::hl7(),
            json!({"op": "copy", "from": "col.nope", "to": "PID.3"}),
        ),
        (
            &Schema::au(),
            &Schema::hl7(),
            json!({"op": "copy", "from": "sample", "to": "PID.3"}),
        ),
        (
            &Schema::hl7(),
            &Schema::au(),
            json!({"op": "copy", "from": "PID.3", "to": "sample_id"}),
        ),
        (
            &Schema::hl7(),
            &columns,
            json!({"op": "copy", "from": "PID.3", "to": "col.nope"}),
        ),
        (
            &Schema::astm(),
            &Schema::astm(),
            json!({"op": "copy", "from": "R.3", "to": "R.1"}),
        ),
        (
            &Schema::astm(),
            &Schema::hl7(),
            json!({"op": "copy", "from": "PID.3", "to": "PID.3"}),
        ),
    ];
    for (source, target, rule) in cases {
        let problems =
            Mapping::compile(&json!({"rules": [rule]}), source, target, "m").expect_err("invalid");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].path.starts_with("m.rules[0]."), "{problems:?}");
    }
    let ok = Mapping::compile(
        &json!({"rules": [{"op": "copy", "from": "col.sample_id", "to": "PID.3"}]}),
        &columns,
        &Schema::hl7(),
        "m",
    );
    assert!(ok.is_ok());
}

#[test]
fn a_document_that_is_not_a_mapping_is_a_problem_not_a_panic() {
    let bad = [
        json!(null),
        json!([]),
        json!("x"),
        json!({}),
        json!({"rules": 5}),
        json!({"rules": [], "x": 1}),
    ];
    for value in bad {
        let result = Mapping::compile(&value, &Schema::hl7(), &Schema::hl7(), "mapping");
        assert!(result.is_err(), "{value}");
    }
    assert!(Mapping::compile(&json!({"rules": []}), &Schema::hl7(), &Schema::hl7(), "m").is_ok());
}

fn rule_field_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        prop::sample::select(vec![
            "PID.3",
            "PID.5.1",
            "MSH.9",
            "MSH.2",
            "OBX[1].5",
            "OBX.5",
            "$.5",
            "$.3.1",
            "$.1",
            "ZZZ.1",
            "ZZZ.2.1.1",
            "PID..",
            "col.x",
            "",
            "$.",
        ])
        .prop_map(Value::from),
        prop::sample::select(vec![
            "OBX", "ZZZ", "MSH", "PID", "codes", "none", "error", "keep", "skip", "default"
        ])
        .prop_map(Value::from),
        prop::sample::select(vec![
            "yyyyMMddHHmmss",
            "yyyy-MM-dd",
            "HH:mm",
            "dd.MM.yy",
            "YYYY",
            "yyyyMMdd"
        ])
        .prop_map(Value::from),
        prop::sample::select(vec![
            "UTC",
            "America/New_York",
            "Asia/Tokyo",
            "Nowhere/Land"
        ])
        .prop_map(Value::from),
        any::<bool>().prop_map(Value::from),
        (-5i64..2000).prop_map(Value::from),
        "\\PC{0,6}".prop_map(Value::from),
        Just(json!([{"from": "PID.5.1"}, {"text": "-"}])),
        Just(json!({"path": "$.5", "op": "exists"})),
        Just(json!([])),
    ]
}

fn random_rule(depth: u32) -> BoxedStrategy<Value> {
    let keys = vec![
        "from",
        "to",
        "value",
        "default",
        "required",
        "table",
        "on_missing",
        "ignore_case",
        "parts",
        "input",
        "output",
        "from_timezone",
        "to_timezone",
        "condition",
        "over",
        "into",
        "start",
    ];
    let ops = vec![
        "copy", "constant", "lookup", "concat", "datetime", "when", "for_each", "sequence", "nope",
    ];
    let flat = (
        prop::sample::select(ops),
        prop::collection::btree_map(prop::sample::select(keys), rule_field_value(), 0..7),
    )
        .prop_map(|(op, fields)| {
            let mut rule = serde_json::Map::new();
            rule.insert("op".to_owned(), json!(op));
            rule.extend(fields.into_iter().map(|(k, v)| (k.to_owned(), v)));
            Value::Object(rule)
        });
    if depth == 0 {
        return flat.boxed();
    }
    let nested = random_rule(depth - 1);
    (
        flat,
        prop::collection::vec(nested, 0..3),
        prop::sample::select(vec!["rules", "then", "else"]),
    )
        .prop_map(|(mut rule, children, key)| {
            if let Some(object) = rule.as_object_mut() {
                object.insert(key.to_owned(), Value::Array(children));
            }
            rule
        })
        .boxed()
}

proptest! {
    #[test]
    fn arbitrary_rules_never_panic_to_compile_or_run(rules in prop::collection::vec(random_rule(2), 0..5)) {
        let mapping = json!({"tables": {"codes": {"GLU": "g", "K": "k"}}, "rules": rules});
        if let Ok(mapping) = Mapping::compile(&mapping, &Schema::hl7(), &Schema::hl7(), "m") {
            let mut target = Hl7Model::new();
            let _ = mapping.apply(&oru(), &mut target);
            let _ = target.encode();
            let mut clone = oru();
            let _ = mapping.apply(&oru(), &mut clone);
        }
    }
}
