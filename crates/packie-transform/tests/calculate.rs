//! `calculate`, `append`, `for_each` in update mode, structured numerics and text steps:
//! exact results, rounding, comparators, invalid operands, group safety and validation.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use packie_transform::{
    AstmModel, Hl7Model, Mapping, Model, Path, Reason, Schema, Syntax, TransformError,
};
use serde_json::{Value, json};

/// A synthetic ASTM message with one R record per value (test codes `T1`, `T2`, …).
fn astm(values: &[&str]) -> AstmModel {
    let mut text = String::from("H|\\^&|||SYN\rP|1||SYN-1\rO|1|SYN-0001\r");
    for (i, value) in values.iter().enumerate() {
        text.push_str(&format!("R|{}|^^^T{}|{value}|mg/dL\r", i + 1, i + 1));
    }
    text.push_str("L|1|N\r");
    AstmModel::parse(text.as_bytes()).expect("synthetic ASTM parses")
}

fn hl7_text(model: &Hl7Model) -> String {
    String::from_utf8(model.encode())
        .expect("ascii")
        .replace('\r', "\n")
}

fn get(model: &dyn Model, syntax: Syntax, path: &str) -> String {
    model
        .get(&Path::parse(syntax, path).expect("path"))
        .map(|text| text.into_owned())
        .unwrap_or_default()
}

/// Runs `rule` (with `"to": "$.5"`) for every R of an ASTM message holding `values`; the
/// OBX-5 of each output occurrence.
fn calc(rule: &Value, values: &[&str]) -> Result<Vec<String>, TransformError> {
    let mut rule = rule.clone();
    rule["op"] = json!("calculate");
    rule["to"] = json!("$.5");
    let mapping = Mapping::compile(
        &json!({"rules": [{"op": "for_each", "over": "R", "into": "OBX", "rules": [rule]}]}),
        &Schema::astm(),
        &Schema::hl7(),
        "mapping",
    )
    .unwrap_or_else(|problems| panic!("expected a valid mapping: {problems:?}"));
    let mut target = Hl7Model::new();
    mapping.apply(&astm(values), &mut target)?;
    Ok((0..values.len())
        .map(|i| get(&target, Syntax::Hl7, &format!("OBX[{i}].5")))
        .collect())
}

fn calc_ok(rule: &Value, values: &[&str]) -> Vec<String> {
    calc(rule, values).unwrap_or_else(|error| panic!("mapping failed: {error}"))
}

#[test]
fn unit_conversion_is_exact_and_rounded_once() {
    let glucose = json!({"formula": {"mul": [{"from": "$.4"}, "0.0555"]}, "scale": 2});
    // 90 × 0.0555 = 4.995 exactly: a tie, rounded half up (binary floats give 4.99).
    assert_eq!(
        calc_ok(&glucose, &["90", "100", "0"]),
        ["5.00", "5.55", "0.00"]
    );
    let creatinine = json!({"formula": {"div": [{"from": "$.4"}, "88.4"]}, "scale": 2});
    assert_eq!(calc_ok(&creatinine, &["88.4", "100"]), ["1.00", "1.13"]);
    let exact = json!({"formula": {"mul": [{"from": "$.4"}, "18.016"]}});
    assert_eq!(calc_ok(&exact, &["5.50", "-0.5"]), ["99.088", "-9.008"]);
}

#[test]
fn rounding_modes() {
    let values = ["2.345", "-2.345", "2.335", "1.005", " 7.899 ", "-0.001"];
    let run = |rounding: &str| {
        calc_ok(
            &json!({"formula": {"from": "$.4"}, "scale": 2, "rounding": rounding}),
            &values,
        )
    };
    assert_eq!(
        run("half_up"),
        ["2.35", "-2.35", "2.34", "1.01", "7.90", "0.00"]
    );
    assert_eq!(
        run("half_even"),
        ["2.34", "-2.34", "2.34", "1.00", "7.90", "0.00"]
    );
    assert_eq!(
        run("truncate"),
        ["2.34", "-2.34", "2.33", "1.00", "7.89", "0.00"]
    );
    // re-scaling pads with zeros
    let pad = json!({"formula": {"from": "$.4"}, "scale": 3});
    assert_eq!(calc_ok(&pad, &["5", "5.5"]), ["5.000", "5.500"]);
}

#[test]
fn results_without_scale_are_exact_in_shortest_form() {
    let add = json!({"formula": {"add": [{"from": "$.4"}, "0.000000001"]}});
    assert_eq!(
        calc_ok(&add, &["12345678901234567890.123456789"]),
        ["12345678901234567890.12345679"]
    );
    let identity = json!({"formula": {"from": "$.4"}});
    assert_eq!(
        calc_ok(
            &identity,
            &["1234567890.123456789012345678", "5.50", "-0", ".5"]
        ),
        ["1234567890.123456789012345678", "5.5", "0", "0.5"]
    );
}

#[test]
fn long_fractions_round_to_any_scale() {
    let rescale = json!({"formula": {"from": "$.4"}, "scale": 12});
    assert_eq!(
        calc_ok(&rescale, &["0.1234567890123456789012345678"]),
        ["0.123456789012"]
    );
    let divide = json!({"formula": {"div": [{"from": "$.4"}, "7.000000000000000000000000001"]}, "scale": 12});
    assert_eq!(
        calc_ok(&divide, &["1", "<1"]),
        ["0.142857142857", "<0.142857142858"]
    );
}

#[test]
fn comparators_are_carried_truthfully() {
    let factor = json!({"formula": {"mul": [{"from": "$.4"}, "0.0555"]}, "scale": 2});
    // <5 → <0.2775 → <0.28; <4.91 → <0.272505 → <0.28 (half up would claim <0.27).
    assert_eq!(
        calc_ok(&factor, &["<5", "<4.91", "<= 5", ">5", ">=4.91"]),
        ["<0.28", "<0.28", "<=0.28", ">0.27", ">=0.27"]
    );
    let truncate = json!({"formula": {"from": "$.4"}, "scale": 1, "rounding": "truncate"});
    assert_eq!(
        calc_ok(&truncate, &["<17.61", ">17.68"]),
        ["<17.7", ">17.6"]
    );
    let reversed = json!({"formula": {"sub": ["1", {"from": "$.4"}]}});
    assert_eq!(calc_ok(&reversed, &["<0.20", ">=3"]), [">0.8", "<=-2"]);
    let negative = json!({"formula": {"mul": [{"from": "$.4"}, -2]}});
    assert_eq!(calc_ok(&negative, &["<3"]), [">-6"]);
    // An exact zero factor gives exactly 0, never `<0`.
    let zero = json!({"formula": {"mul": [{"from": "$.4"}, 0]}});
    assert_eq!(calc_ok(&zero, &["<3", ">3"]), ["0", "0"]);
}

#[test]
fn comparators_that_cannot_be_carried_are_invalid() {
    for formula in [
        json!({"div": ["1", {"from": "$.4"}]}),
        json!({"min": [{"from": "$.4"}, "1"]}),
        json!({"abs": {"from": "$.4"}}),
        json!({"add": [{"from": "$.4"}, {"from": "$.4"}]}),
    ] {
        let error = calc(&json!({"formula": formula, "scale": 1}), &["<3"])
            .expect_err("comparator not carried");
        assert_eq!(error.reason, Reason::Comparator, "{formula}");
        assert_eq!(error.path, "$.5");
    }
    let reject = json!({"formula": {"from": "$.4"}, "comparator": "reject"});
    let error = calc(&reject, &["<3"]).expect_err("rejected");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::NotNumeric, "$.4")
    );
}

#[test]
fn invalid_operands_follow_on_invalid_and_errors_never_show_values() {
    let base = json!({"formula": {"mul": [{"from": "$.4"}, "2"]}});
    let error = calc(&base, &["NEG"]).expect_err("not a number");
    assert_eq!(error.reason, Reason::NotNumeric);
    assert_eq!(
        error.to_string(),
        "rule rules[0].rules[0] (calculate) at path $.4: value is not a decimal number"
    );
    for text in ["1e3", "1,000", "5 mmol", "<", "--1"] {
        assert_eq!(
            calc(&base, &[text]).expect_err(text).reason,
            Reason::NotNumeric
        );
    }
    let with = |mode: Value| {
        let mut rule = base.clone();
        rule["on_invalid"] = mode;
        rule
    };
    assert_eq!(calc_ok(&with(json!("skip")), &["NEG", "2"]), ["", "4"]);
    assert_eq!(calc_ok(&with(json!("keep")), &["NEG", "2"]), ["NEG", "4"]);
    let mut default = with(json!("default"));
    default["default"] = json!("ERR");
    assert_eq!(calc_ok(&default, &["NEG"]), ["ERR"]);

    let divide = json!({"formula": {"div": [{"from": "$.4"}, {"sub": ["1", "1"]}]}, "scale": 2});
    let error = calc(&divide, &["90"]).expect_err("division by zero");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::DivisionByZero, "$.5")
    );
    let huge = json!({"formula": {"mul": [{"from": "$.4"}, "10"]}});
    let error = calc(&huge, &["99999999999999999999999999999999999999"]).expect_err("range");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::OutOfRange, "$.5")
    );
}

#[test]
fn empty_operands_write_nothing_unless_required() {
    let rule = json!({"formula": {"add": [{"from": "$.4"}, "1"]}});
    assert_eq!(calc_ok(&rule, &["", "  ", "1"]), ["", "", "2"]);
    let mut required = rule;
    required["required"] = json!(true);
    let error = calc(&required, &[""]).expect_err("required");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::RequiredMissing, "$.4")
    );
}

#[test]
fn clamp_and_operand_steps() {
    let clamp = json!({"formula": {"max": [{"from": "$.4"}, 0]}});
    assert_eq!(calc_ok(&clamp, &["-1.5", "2"]), ["0", "2"]);
    let comma = json!({
        "formula": {"from": "$.4", "steps": [{"step": "replace", "find": ",", "with": "."}]},
        "scale": 1, "rounding": "half_even"
    });
    assert_eq!(calc_ok(&comma, &["4,25", "4,35"]), ["4.2", "4.4"]);
}

/// A synthetic HL7 result message: one patient, one order, chemistry results (CREA is a
/// structured numeric with a comparator).
const CHEM: &[u8] = b"MSH|^~\\&|APP|FAC|LIS|LAB|20261012120000||ORU^R01|M1|P|2.5.1\r\
PID|1||SYN-1\r\
OBR|1||SYN-0001|PANEL\r\
OBX|1|NM|NA||140|mmol/L\r\
OBX|2|NM|CL||104|mmol/L\r\
OBX|3|NM|HCO3||24|mmol/L\r\
OBX|4|NM|GLU||90|mg/dL\r\
OBX|5|SN|CREA||<^0.20|mg/dL\r\
OBX|6|SN|TITER||^1^:^128\r";

/// Runs `rules` over [`CHEM`] (or `source`) into a copy of it, as a same-protocol HL7
/// destination does.
fn same_hl7(source: &[u8], rules: &Value) -> Result<String, TransformError> {
    let source = Hl7Model::parse(source).expect("synthetic message parses");
    let mapping = Mapping::compile(
        &json!({"rules": rules}),
        &Schema::hl7(),
        &Schema::hl7(),
        "m",
    )
    .unwrap_or_else(|problems| panic!("expected a valid mapping: {problems:?}"));
    let mut target = source.clone();
    mapping.apply(&source, &mut target)?;
    Ok(hl7_text(&target))
}

fn chem_lines() -> Vec<String> {
    String::from_utf8(CHEM.to_vec())
        .expect("ascii")
        .split('\r')
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn update_mode_rewrites_each_occurrence_in_place() {
    let rules = json!([{ "op": "for_each", "over": "OBX", "into": "OBX", "mode": "update", "rules": [
        { "op": "when", "condition": {"path": "$.3", "op": "equals", "value": "GLU"}, "then": [
            { "op": "calculate", "formula": {"mul": [{"from": "$.5"}, "0.0555"]}, "to": "$.5", "scale": 2 },
            { "op": "constant", "value": "mmol/L", "to": "$.6" } ] },
        { "op": "sequence", "to": "$.1", "start": 11 } ] }]);
    let out = same_hl7(CHEM, &rules).expect("mapping runs");
    let mut expected = chem_lines();
    for (i, line) in expected.iter_mut().enumerate().skip(3) {
        *line = line.replacen(&format!("OBX|{}|", i - 2), &format!("OBX|{}|", i + 8), 1);
    }
    expected[6] = "OBX|14|NM|GLU||5.00|mmol/L".to_owned();
    assert_eq!(out, expected.join("\n") + "\n");
    // `create` (the default) appends new occurrences after the copied ones.
    let mut create = rules;
    create[0]["mode"] = json!("create");
    let out = same_hl7(CHEM, &create).expect("mapping runs");
    assert_eq!(out.matches("\nOBX|").count(), 12);
}

#[test]
fn structured_numeric_values_are_read_and_written_by_component() {
    let rules = json!([{ "op": "for_each", "over": "OBX", "into": "OBX", "mode": "update", "rules": [
        { "op": "when", "condition": {"path": "$.2", "op": "equals", "value": "SN"}, "then": [
            { "op": "calculate", "notation": "sn", "formula": {"mul": [{"from": "$.5"}, "88.4"]},
              "to": "$.5", "scale": 1, "on_invalid": "keep" } ] } ] }]);
    let out = same_hl7(CHEM, &rules).expect("mapping runs");
    let mut expected = chem_lines();
    // 0.20 × 88.4 = 17.68; a `<` bound is rounded up: <17.7. The ratio is kept unchanged.
    expected[7] = "OBX|5|SN|CREA||<^17.7|mg/dL".to_owned();
    assert_eq!(out, expected.join("\n") + "\n");

    let plain = b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1\rOBX|1|SN|X||^5\rOBX|2|SN|Y||=^2.5\rOBX|3|SN|Z||>=^3\r";
    let rules = json!([{ "op": "for_each", "over": "OBX", "into": "OBX", "mode": "update", "rules": [
        { "op": "calculate", "notation": "sn", "formula": {"mul": [{"from": "$.5"}, "2"]}, "to": "$.5" } ] }]);
    assert_eq!(
        same_hl7(plain, &rules).expect("mapping runs"),
        "MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\nPID|1\nOBX|1|SN|X||^10\nOBX|2|SN|Y||^5\nOBX|3|SN|Z||>=^6\n"
    );
    let unknown = b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rOBX|1|SN|X||<>^5\r";
    let error = same_hl7(unknown, &rules).expect_err("<> is not a comparator");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::NotNumeric, "$.5")
    );
}

fn find(code: &str) -> Value {
    json!({"find": "OBX", "where": {"path": "$.3", "op": "equals", "value": code}, "from": "$.5"})
}

fn anion_gap(codes: [&str; 3]) -> Value {
    json!([{ "op": "append", "into": "OBX", "rules": [
        { "op": "calculate", "to": "$.5", "on_invalid": "skip",
          "formula": {"sub": [find(codes[0]), find(codes[1]), find(codes[2])]} },
        { "op": "sequence", "to": "$.1" },
        { "op": "constant", "value": "NM", "to": "$.2" },
        { "op": "constant", "value": "AG^Anion gap", "to": "$.3" },
        { "op": "copy", "from": "OBX.6", "to": "$.6" } ] }])
}

#[test]
fn append_adds_a_calculated_result_only_when_every_rule_writes() {
    let out = same_hl7(CHEM, &anion_gap(["NA", "CL", "HCO3"])).expect("mapping runs");
    let mut expected = chem_lines();
    expected.push("OBX|7|NM|AG^Anion gap||12|mmol/L".to_owned());
    assert_eq!(out, expected.join("\n") + "\n");
    // A missing input: nothing is appended, not even the constants.
    let out = same_hl7(CHEM, &anion_gap(["NA", "CL", "XX"])).expect("mapping runs");
    assert_eq!(out, chem_lines().join("\n") + "\n");
}

#[test]
fn find_and_append_refuse_messages_with_several_groups() {
    let two_orders = b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||SYN-1\rOBR|1||S1\rOBX|1|NM|NA||140\rOBX|2|NM|CL||104\r\
OBR|2||S2\rOBX|3|NM|HCO3||24\r";
    let error = same_hl7(two_orders, &anion_gap(["NA", "CL", "HCO3"])).expect_err("ambiguous");
    assert_eq!(error.reason, Reason::MultipleGroups);
    assert_eq!(
        error.to_string(),
        "rule rules[0] (append) at path OBX: \
         the message holds more than one patient, order or specimen group"
    );
    let constant_only = json!([{ "op": "append", "into": "NTE", "rules": [
        { "op": "constant", "value": "note", "to": "$.3" } ] }]);
    let error = same_hl7(two_orders, &constant_only).expect_err("ambiguous placement");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::MultipleGroups, "NTE")
    );
    // `on_invalid` does not hide it.
    let rules = json!([{ "op": "for_each", "over": "OBX", "into": "OBX", "mode": "update", "rules": [
        { "op": "calculate", "to": "$.7", "on_invalid": "skip",
          "formula": {"div": [{"from": "$.5"}, find("NA")]}, "scale": 3 } ] }]);
    let error = same_hl7(two_orders, &rules).expect_err("ambiguous");
    assert_eq!(
        error.to_string(),
        "rule rules[0].rules[0] (calculate) at path $.5: \
         the message holds more than one patient, order or specimen group"
    );
    assert!(same_hl7(CHEM, &rules).is_ok());
}

#[test]
fn steps_follow_every_from() {
    let source = b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||000120^^^HOSP||doe^jane\rOBX|1|NM|GLU/2||5.5||3.9-6.1\r\
OBX|2|DT|DATE||2026-10-12\r";
    let rules = json!([
        { "op": "copy", "from": "PID.3.1", "to": "PID.2",
          "steps": [{"step": "strip_leading", "chars": "0"}] },
        { "op": "lookup", "from": "OBX.3", "table": "codes", "to": "OBX.4",
          "steps": [{"step": "split", "separator": "/", "take": 0}] },
        { "op": "concat", "to": "PID.8", "parts": [
            {"from": "PID.5.2", "steps": [{"step": "slice", "start": 0, "length": 1}, {"step": "upper"}]},
            {"text": "."} ] },
        { "op": "datetime", "from": "OBX[1].5", "to": "OBX[1].14", "input": "yyyyMMdd", "output": "yyyyMMdd",
          "steps": [{"step": "replace", "find": "-", "with": ""}] },
        { "op": "copy", "from": "OBX.7", "to": "OBX.9",
          "steps": [{"step": "split", "separator": "-", "take": -1}] }
    ]);
    let source = Hl7Model::parse(source).expect("parses");
    let mapping = Mapping::compile(
        &json!({"tables": {"codes": {"GLU": "2345-7"}}, "rules": rules}),
        &Schema::hl7(),
        &Schema::hl7(),
        "m",
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    let mut target = Hl7Model::new();
    mapping.apply(&source, &mut target).expect("runs");
    assert_eq!(
        hl7_text(&target),
        "MSH|^~\\&\nPID||120||||||J.\nOBX||||2345-7|||||6.1\nOBX||||||||||||||20261012\n"
    );
}

fn problems(mapping: &Value, source: &Schema, target: &Schema) -> Vec<String> {
    Mapping::compile(mapping, source, target, "m")
        .expect_err("invalid")
        .into_iter()
        .map(|p| p.to_string())
        .collect()
}

#[test]
fn validation_reports_calculation_problems_with_paths() {
    let deep = (0..8).fold(json!({"from": "$.4"}), |inner, _| json!({"abs": inner}));
    let wide: Vec<Value> = (0..16).map(|_| json!({"add": ["1", "1", "1"]})).collect();
    let mapping = json!({"rules": [{"op": "for_each", "over": "R", "into": "OBX", "rules": [
        {"op": "calculate", "to": "$.5", "formula": {"div": [{"from": "$.4"}, "x"]}},
        {"op": "calculate", "to": "$.5", "formula": {"from": "$.4"}, "rounding": "half_up"},
        {"op": "calculate", "to": "$.5", "formula": {"add": [{"from": "$.4"}, {"from": "$.5"}]}, "on_invalid": "keep"},
        {"op": "calculate", "to": "$.5", "formula": {"add": ["1"]}, "on_invalid": "default"},
        {"op": "calculate", "to": "$.5", "formula": {"pow": ["1", "2"]}, "default": "x", "scale": 13},
        {"op": "calculate", "to": "$.5", "formula": deep, "comparator": "maybe"},
        {"op": "calculate", "to": "$.5", "formula": {"add": wide}},
        {"op": "calculate", "to": "$.5", "formula": {"from": "$.4", "find": "R"}},
        {"op": "calculate", "to": "$.5", "formula": {"find": "Z", "from": "R.4"}},
        {"op": "calculate", "to": "$.5", "formula": {"from": "$.4"}, "notation": "sn"},
        {"op": "append", "into": "OBX", "rules": []}
    ]}]});
    assert_eq!(
        problems(&mapping, &Schema::astm(), &Schema::hl7()),
        [
            "m.rules[0].rules[0].formula.div[1]: must be a decimal number",
            "m.rules[0].rules[0].scale: is required when the formula divides",
            "m.rules[0].rules[1].rounding: is only used with `scale`",
            "m.rules[0].rules[2].on_invalid: `keep` needs a formula with exactly one `from` or `find` operand",
            "m.rules[0].rules[3].formula.add: must be an array of 2 to 16 formulas",
            "m.rules[0].rules[3].default: is required with on_invalid `default`",
            "m.rules[0].rules[4].formula.pow: unknown operator; a formula is a decimal, {\"from\": …}, {\"find\": …} or one of add, sub, mul, div, min, max, abs",
            "m.rules[0].rules[4].scale: must be an integer from 0 to 12",
            "m.rules[0].rules[4].default: is only used with on_invalid `default`",
            "m.rules[0].rules[5].formula.abs.abs.abs.abs.abs.abs.abs.abs: formulas are nested too deeply (at most 8 levels)",
            "m.rules[0].rules[5].comparator: must be one of carry, reject",
            "m.rules[0].rules[6].formula.add[15].add[2]: a formula has at most 64 parts",
            "m.rules[0].rules[7].formula.where: is required",
            "m.rules[0].rules[8].formula.find: expected an ASTM record letter (H, P, O, R, C, Q, M, L)",
            "m.rules[0].rules[8].formula.where: is required",
            "m.rules[0].rules[8].formula.from: inside find, `from` is a `$.` path of the found occurrence",
            "m.rules[0].rules[9].notation: `sn` needs an HL7 source and an HL7 target",
            "m.rules[0].rules[10]: append cannot be inside for_each or append",
        ]
    );
}

#[test]
fn validation_reports_append_update_and_sn_problems() {
    let hl7 = Schema::hl7();
    let mapping = json!({"rules": [
        {"op": "append", "into": "OBX", "rules": [
            {"op": "constant", "value": "x", "to": "OBX.5"},
            {"op": "copy", "from": "$.5", "to": "$.5"},
            {"op": "when", "condition": {"path": "PID.3", "op": "exists"}, "then": []},
            {"op": "append", "into": "NTE", "rules": [{"op": "constant", "value": "x", "to": "$.1"}]}
        ]},
        {"op": "append", "into": "MSH", "rules": [{"op": "constant", "value": "x", "to": "$.3"}]},
        {"op": "for_each", "over": "OBX", "into": "NTE", "mode": "update", "rules": []},
        {"op": "for_each", "over": "OBX", "into": "OBX", "mode": "replace", "rules": []},
        {"op": "calculate", "notation": "sn", "formula": {"from": "OBX.5.2"}, "to": "OBX.5.1"}
    ]});
    assert_eq!(
        problems(&mapping, &hl7, &hl7),
        [
            "m.rules[0].rules[0].to: inside append, `to` is a `$.` path of the new occurrence",
            "m.rules[0].rules[1].from: `$.` paths are only valid inside for_each",
            "m.rules[0].rules[2].op: append holds only rules that write",
            "m.rules[0].rules[3]: append cannot be inside for_each or append",
            "m.rules[1].into: a message has one MSH",
            "m.rules[2].mode: mode `update` needs `into` equal to `over`",
            "m.rules[3].mode: must be one of create, update, repeat",
            "m.rules[4].formula.from: with notation `sn` the path addresses a field or a repetition, not a component",
            "m.rules[4].to: with notation `sn` the path addresses a field or a repetition, not a component",
        ]
    );
    let update = json!({"rules": [{"op": "for_each", "over": "R", "into": "R", "mode": "update", "rules": []}]});
    assert_eq!(
        problems(&update, &Schema::astm(), &hl7),
        [
            "m.rules[0].into: expected a three-character segment name such as OBX",
            "m.rules[0].mode: mode `update` needs an HL7 or ASTM target of the source protocol",
        ]
    );
    assert!(Mapping::compile(&update, &Schema::astm(), &Schema::astm(), "m").is_ok());
}

#[test]
fn numeric_constants_must_have_arrived_exactly() {
    let rule = |formula: Value, default: Value| {
        json!({"rules": [{"op": "for_each", "over": "R", "into": "OBX", "rules": [
            {"op": "calculate", "to": "$.5", "formula": formula,
             "on_invalid": "default", "default": default}]}]})
    };
    let hint = "write a fractional or large constant as a decimal string, for example \"0.0555\"";
    let found = problems(
        &rule(
            json!({"add": [88.4, 1.000_000_000_000_000_1, 9_007_199_254_740_992_u64, {"from": "$.4"}]}),
            json!(0.5),
        ),
        &Schema::astm(),
        &Schema::hl7(),
    );
    assert_eq!(
        found,
        [
            format!("m.rules[0].rules[0].formula.add[0]: {hint}"),
            format!("m.rules[0].rules[0].formula.add[1]: {hint}"),
            format!("m.rules[0].rules[0].formula.add[2]: {hint}"),
            format!("m.rules[0].rules[0].default: {hint}"),
        ]
    );
    // Safe integers as numbers, everything else as strings, is exact.
    let exact = rule(
        json!({"add": [{"from": "$.4"}, "1.0000000000000001", -9_007_199_254_740_991_i64, 9_007_199_254_740_991_u64]}),
        json!(7),
    );
    let mapping = Mapping::compile(&exact, &Schema::astm(), &Schema::hl7(), "m").expect("valid");
    let mut target = Hl7Model::new();
    mapping
        .apply(&astm(&["0", "x"]), &mut target)
        .expect("runs");
    assert_eq!(get(&target, Syntax::Hl7, "OBX.5"), "1.0000000000000001");
    assert_eq!(get(&target, Syntax::Hl7, "OBX[1].5"), "7");
    // Generic text fields keep accepting numbers as before.
    let generic = json!({"rules": [{"op": "constant", "value": 1.5, "to": "PID.3"}]});
    assert!(Mapping::compile(&generic, &Schema::hl7(), &Schema::hl7(), "m").is_ok());
}

/// An ASTM transmission with two patients, each with one order and one result.
const TWO_SAMPLES: &[u8] = b"H|\\^&|||SYN\rP|1||SYN-1\rO|1|SYN-0001\rR|1|^^^GLU|90\r\
P|2||SYN-2\rO|1|SYN-0002\rR|1|^^^GLU|100\rL|1|N\r";

fn single_sample_preset(require: bool) -> Mapping {
    Mapping::compile(
        &json!({"require_single_group": require, "rules": [
            {"op": "copy", "from": "O.3", "to": "OBR.3"},
            {"op": "for_each", "over": "R", "into": "OBX", "rules": [
                {"op": "copy", "from": "$.4", "to": "$.5"}]}]}),
        &Schema::astm(),
        &Schema::hl7(),
        "m",
    )
    .expect("valid")
}

#[test]
fn require_single_group_refuses_several_samples_before_any_rule() {
    let source = AstmModel::parse(TWO_SAMPLES).expect("parses");
    let mut target = Hl7Model::new();
    let error = single_sample_preset(true)
        .apply(&source, &mut target)
        .expect_err("two patients");
    assert_eq!(error.reason, Reason::MultipleGroups);
    assert_eq!(
        error.to_string(),
        "rule require_single_group (mapping) at path P: \
         the message holds more than one patient, order or specimen group"
    );
    assert_eq!(hl7_text(&target), "MSH|^~\\&\n", "no rule ran");
    // Without the flag the mapping runs as before (and attributes both results to SYN-0001).
    let mut target = Hl7Model::new();
    single_sample_preset(false)
        .apply(&source, &mut target)
        .expect("runs");
    assert_eq!(
        hl7_text(&target),
        "MSH|^~\\&\nOBR|||SYN-0001\nOBX|||||90\nOBX|||||100\n"
    );
    let two_orders = Hl7Model::parse(
        b"MSH|^~\\&|A|B|C|D|20261012120000||ORM^O01|M1|P|2.5.1\rPID|1\rORC|NW\rOBR|1\rORC|NW\rOBR|2\r",
    )
    .expect("parses");
    let hl7 = Mapping::compile(
        &json!({"require_single_group": true, "rules": [{"op": "copy", "from": "OBR.1", "to": "OBR.2"}]}),
        &Schema::hl7(),
        &Schema::hl7(),
        "m",
    )
    .expect("valid");
    let mut target = two_orders.clone();
    let error = hl7.apply(&two_orders, &mut target).expect_err("two orders");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::MultipleGroups, "OBR")
    );
}

#[test]
fn require_single_group_passes_one_sample_and_is_validated() {
    let mut target = Hl7Model::new();
    single_sample_preset(true)
        .apply(&astm(&["90", "<5"]), &mut target)
        .expect("one sample");
    assert_eq!(
        hl7_text(&target),
        "MSH|^~\\&\nOBR|||SYN-0001\nOBX|||||90\nOBX|||||<5\n"
    );
    let invalid = json!({"require_single_group": "yes", "rules": []});
    assert_eq!(
        problems(&invalid, &Schema::astm(), &Schema::hl7()),
        ["m.require_single_group: must be true or false"]
    );
}

/// Three replace steps that multiply a one-character field by 64 each (262,144 bytes).
fn expanding() -> Value {
    let step = json!({"step": "replace", "find": "0", "with": "0".repeat(64)});
    json!([step.clone(), step.clone(), step])
}

#[test]
fn expanding_steps_are_a_value_free_rule_error() {
    let source = b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||0||DOE^JANE\r";
    let run = |rule: Value| {
        let mapping = Mapping::compile(
            &json!({"tables": {"t": {"0": "zero"}}, "rules": [rule]}),
            &Schema::hl7(),
            &Schema::hl7(),
            "m",
        )
        .unwrap_or_else(|problems| panic!("{problems:?}"));
        let mut target = Hl7Model::new();
        mapping
            .apply(&Hl7Model::parse(source).expect("parses"), &mut target)
            .expect_err("too long")
    };
    let error = run(json!({"op": "copy", "from": "PID.3", "to": "PID.4", "steps": expanding()}));
    assert_eq!(
        error.to_string(),
        "rule rules[0] (copy) at path PID.3: the text steps would make the value longer than allowed"
    );
    assert!(!error.to_string().contains("000"));
    // No fallback hides it: not a lookup miss, not an empty concat part.
    let lookup = json!({"op": "lookup", "from": "PID.3", "table": "t", "to": "PID.4",
                        "on_missing": "skip", "steps": expanding()});
    assert_eq!(run(lookup).reason, Reason::StepsTooLong);
    let concat = json!({"op": "concat", "to": "PID.4", "parts": [{"text": "x"}, {"from": "PID.3", "steps": expanding()}]});
    assert_eq!(run(concat).path, "PID.3");
}

#[test]
fn expanding_operand_steps_follow_on_invalid_except_keep() {
    let rule = |on_invalid: &str| {
        let mut rule = json!({"formula": {"add": [{"from": "$.4", "steps": expanding()}, "1"]},
                              "on_invalid": on_invalid});
        if on_invalid == "default" {
            rule["default"] = json!("ERR");
        }
        rule
    };
    let error = calc(&rule("error"), &["0"]).expect_err("too long");
    assert_eq!(
        error.to_string(),
        "rule rules[0].rules[0] (calculate) at path $.4: the text steps would make the value longer than allowed"
    );
    assert_eq!(calc_ok(&rule("skip"), &["0", "5"]), ["", "6"]);
    assert_eq!(calc_ok(&rule("default"), &["0", "5"]), ["ERR", "6"]);
    // `keep` would write the very text the steps could not produce: an error, not a retry.
    let error = calc(&rule("keep"), &["0"]).expect_err("keep cannot keep it");
    assert_eq!(
        (error.reason, error.path.as_str()),
        (Reason::StepsTooLong, "$.4")
    );
    assert_eq!(calc_ok(&rule("keep"), &["5", "NEG"]), ["6", "NEG"]);
}

#[test]
fn large_fields_pass_through_non_growing_steps() {
    let note = "ab-".repeat(40_000);
    let source =
        format!("MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rOBX|1|TX|NOTE||{note}\r");
    let mapping = Mapping::compile(
        &json!({"rules": [{"op": "copy", "from": "OBX.5", "to": "OBX.5",
                           "steps": [{"step": "replace", "find": "-", "with": "+"}, {"step": "upper"}]}]}),
        &Schema::hl7(),
        &Schema::hl7(),
        "m",
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"));
    let mut target = Hl7Model::new();
    mapping
        .apply(
            &Hl7Model::parse(source.as_bytes()).expect("parses"),
            &mut target,
        )
        .expect("same length as the field");
    assert_eq!(get(&target, Syntax::Hl7, "OBX.5"), "AB+".repeat(40_000));
}
