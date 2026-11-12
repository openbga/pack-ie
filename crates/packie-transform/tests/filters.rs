//! Filter expressions: every operator, combinations, validation, and property tests that
//! compilation and evaluation never panic.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod support;

use packie_transform::{AstmModel, AuModel, Filter, Filters, Hl7Model, Model, RecordModel, Schema};
use proptest::prelude::*;
use serde_json::{Value, json};
use support::{au_sample, oru};

fn passes(expression: &Value) -> bool {
    Filter::compile(expression, &Schema::hl7(), "f")
        .unwrap_or_else(|problems| panic!("expected a valid filter: {problems:?}"))
        .matches(&oru())
}

fn cmp(path: &str, op: &str, value: Value) -> Value {
    let mut comparison = json!({ "path": path, "op": op });
    comparison["value"] = value;
    comparison
}

fn check(cases: &[(Value, bool)]) {
    for (expression, expected) in cases {
        assert_eq!(passes(expression), *expected, "{expression}");
    }
}

#[test]
fn equals_and_not_equals() {
    check(&[
        (cmp("MSH.9.1", "equals", json!("ORU")), true),
        (cmp("MSH.9.1", "equals", json!("oru")), false),
        (cmp("MSH.9", "equals", json!("ORU^R01")), true),
        (cmp("MSH.9.1", "not_equals", json!("ORM")), true),
        (cmp("MSH.9.1", "not_equals", json!("ORU")), false),
        // a missing field is empty text: it is not equal to anything non-empty
        (cmp("PID.99", "not_equals", json!("x")), true),
        (cmp("PID.99", "equals", json!("x")), false),
        (cmp("ZZZ.1", "not_equals", json!("x")), true),
        // numbers are compared as their text
        (cmp("PID.1", "equals", json!(1)), true),
    ]);
    let ignore = json!({"path": "MSH.9.1", "op": "equals", "value": "oru", "ignore_case": true});
    assert!(passes(&ignore));
}

#[test]
fn in_and_not_in() {
    check(&[
        (cmp("OBX.3.1", "in", json!(["K", "GLU"])), true),
        (cmp("OBX.3.1", "in", json!(["K", "NA"])), false),
        (cmp("OBX.3.1", "not_in", json!(["K", "NA"])), true),
        (cmp("OBX.3.1", "not_in", json!(["GLU"])), false),
        (cmp("PID.99", "in", json!(["a"])), false),
        (cmp("PID.99", "not_in", json!(["a"])), true),
        (cmp("PID.1", "in", json!([1, 2])), true),
    ]);
    let ignore = json!({"path": "OBX.3.1", "op": "in", "value": ["glu"], "ignore_case": true});
    assert!(passes(&ignore));
    let ignore = json!({"path": "OBX.3.1", "op": "not_in", "value": ["glu"], "ignore_case": true});
    assert!(!passes(&ignore));
}

#[test]
fn starts_with_and_contains() {
    check(&[
        (cmp("PID.5", "starts_with", json!("DOE")), true),
        (cmp("PID.5", "starts_with", json!("JANE")), false),
        (cmp("PID.5", "contains", json!("^J")), true),
        (cmp("PID.5", "contains", json!("SMITH")), false),
        (cmp("PID.99", "contains", json!("x")), false),
    ]);
    let ignore = json!({"path": "PID.5", "op": "starts_with", "value": "doe", "ignore_case": true});
    assert!(passes(&ignore));
    let ignore = json!({"path": "PID.5", "op": "contains", "value": "jane", "ignore_case": true});
    assert!(passes(&ignore));
}

#[test]
fn matches_searches_unless_anchored() {
    check(&[
        (cmp("PID.5", "matches", json!("^DOE\\^J")), true),
        (cmp("PID.5", "matches", json!("JANE$")), true),
        (cmp("PID.5", "matches", json!("OE")), true),
        (cmp("PID.5", "matches", json!("^JANE")), false),
        (cmp("PID.5", "matches", json!("^doe")), false),
        (cmp("OBX.5", "matches", json!("^\\d+\\.\\d+$")), true),
        (cmp("PID.99", "matches", json!("^$")), true),
    ]);
    let ignore = json!({"path": "PID.5", "op": "matches", "value": "^doe", "ignore_case": true});
    assert!(passes(&ignore));
}

#[test]
fn matches_uses_a_linear_time_engine() {
    let expression = cmp("PID.5", "matches", json!("(a*)*(b*)*(c*)*$"));
    assert!(passes(&expression));
    let filter = Filter::compile(
        &cmp("PID.5", "matches", json!("(x+x+)+y")),
        &Schema::hl7(),
        "f",
    )
    .expect("valid");
    let mut model = Hl7Model::new();
    model.set_text("PID.5", &"x".repeat(50_000));
    assert!(!filter.matches(&model));
}

#[test]
fn exists_and_not_exists() {
    check(&[
        (json!({"path": "PID.3.1", "op": "exists"}), true),
        (json!({"path": "PID.4", "op": "exists"}), false),
        (json!({"path": "PID.99", "op": "exists"}), false),
        (json!({"path": "ZZZ.1", "op": "exists"}), false),
        (json!({"path": "PID.3.1", "op": "not_exists"}), false),
        (json!({"path": "PID.4", "op": "not_exists"}), true),
        (json!({"path": "PID.99", "op": "not_exists"}), true),
        (json!({"path": "OBX[1].8", "op": "not_exists"}), true),
    ]);
}

#[test]
fn numeric_operators_compare_exact_decimals() {
    check(&[
        (cmp("OBX.5", "gt", json!("5.49")), true),
        (cmp("OBX.5", "gt", json!("5.5")), false),
        (cmp("OBX.5", "gte", json!("5.5")), true),
        (cmp("OBX.5", "gte", json!(5.51)), false),
        (cmp("OBX.5", "lt", json!("5.500001")), true),
        (cmp("OBX.5", "lt", json!("5.5")), false),
        (cmp("OBX.5", "lte", json!("5.5")), true),
        (cmp("OBX.5", "lte", json!(5.49)), false),
        (cmp("OBX.5", "gt", json!(-100)), true),
        (cmp("OBX.5", "gt", json!("5.4999999999999999999999")), true),
        // non-numeric operands compare false, whichever the operator
        (cmp("OBX[1].5", "gt", json!("0")), false),
        (cmp("OBX[1].5", "lt", json!("100")), false),
        (cmp("OBX[1].5", "gte", json!("0")), false),
        (cmp("OBX[1].5", "lte", json!("100")), false),
        (cmp("PID.99", "gt", json!("-1")), false),
        (cmp("PID.5", "lt", json!("1")), false),
    ]);
}

#[test]
fn timestamps_of_one_format_order_as_numbers() {
    check(&[
        (cmp("MSH.7", "gte", json!("20261012000000")), true),
        (cmp("MSH.7", "lt", json!("20261012120000")), false),
        (cmp("MSH.7", "lt", json!("20261013000000")), true),
    ]);
}

#[test]
fn boolean_combinations() {
    let yes = cmp("MSH.9.1", "equals", json!("ORU"));
    let no = cmp("MSH.9.1", "equals", json!("ORM"));
    check(&[
        (json!({"all": [yes, yes]}), true),
        (json!({"all": [yes, no]}), false),
        (json!({"any": [no, yes]}), true),
        (json!({"any": [no, no]}), false),
        (json!({"not": no}), true),
        (json!({"not": yes}), false),
        (json!({"all": [yes, {"any": [no, {"not": no}]}]}), true),
    ]);
}

#[test]
fn filter_lists_require_every_filter() {
    let schema = Schema::hl7();
    let pass = cmp("MSH.9.1", "equals", json!("ORU"));
    let fail = cmp("MSH.9.1", "equals", json!("ORM"));
    let model = oru();
    let all = Filters::compile(&[pass.clone(), pass.clone()], &schema, "filters").expect("valid");
    assert!(all.passes(&model));
    assert_eq!(all.len(), 2);
    let one_fails = Filters::compile(&[pass, fail], &schema, "filters").expect("valid");
    assert!(!one_fails.passes(&model));
    let none = Filters::compile(&[], &schema, "filters").expect("valid");
    assert!(none.is_empty() && none.passes(&model));
}

#[test]
fn filters_read_every_protocol_model() {
    let astm = AstmModel::parse(
        b"H|\\^&|||CHEM^1.0\rP|1|PRAC-1\rO|1|SYN-1\rR|1|^^^GLU|5.5|mmol/L||H\rL|1|N\r",
    )
    .expect("synthetic message parses");
    let expression = json!({"all": [
        cmp("R.3.4", "equals", json!("GLU")),
        cmp("R.4", "gt", json!("5")),
        cmp("O.3", "starts_with", json!("SYN")),
        cmp("R.7", "in", json!(["H", "L"])),
    ]});
    let filter = Filter::compile(&expression, &Schema::astm(), "f").expect("valid");
    assert!(filter.matches(&astm));

    let record = RecordModel::from_row([("sample_id", "S-9"), ("result", "7.25")]);
    let schema = Schema::delimited(["sample_id", "result"]);
    let expression = json!({"all": [
        cmp("col.sample_id", "matches", json!("^S-\\d$")),
        cmp("col.result", "gte", json!("7.25")),
    ]});
    assert!(
        Filter::compile(&expression, &schema, "f")
            .expect("valid")
            .matches(&record)
    );

    let au = AuModel::from_results(&au_sample());
    let expression = json!({"all": [
        cmp("sample_id", "equals", json!("SYN0042")),
        cmp("test[1].value", "gt", json!("99")),
        json!({"path": "test.flags", "op": "not_exists"}),
    ]});
    assert!(
        Filter::compile(&expression, &Schema::au(), "f")
            .expect("valid")
            .matches(&au)
    );
}

#[test]
fn a_model_of_another_protocol_reads_as_empty() {
    let filter = Filter::compile(&cmp("MSH.9.1", "equals", json!("ORU")), &Schema::hl7(), "f")
        .expect("valid");
    assert!(!filter.matches(&RecordModel::new()));
}

#[test]
fn validation_reports_every_problem_with_its_path() {
    let schema = Schema::delimited(["sample_id"]);
    let bad = json!([
        {"path": "col.sample_id", "op": "between", "value": 1},
        {"path": "col..x", "op": "equals", "value": "x"},
        {"path": "col.unknown", "op": "equals", "value": "x"},
        {"all": []},
        {"path": "col.sample_id", "op": "exists", "value": "x"},
        {"path": "col.sample_id", "op": "gt", "value": "abc"},
        {"path": "col.sample_id", "op": "matches", "value": "(unclosed"},
        {"path": "col.sample_id", "op": "equals"},
        {"path": "$.sample_id", "op": "equals", "value": "x"},
        {"path": "col.sample_id", "op": "equals", "value": "x", "extra": 1},
        {"not": {"path": "col.sample_id"}},
        {"path": "col.sample_id", "op": "in", "value": []},
        {"path": "col.sample_id", "op": "in", "value": [true]},
        {"path": "col.sample_id", "op": "equals", "value": "x", "ignore_case": "yes"},
        {"any": [{"path": 5, "op": "equals", "value": "x"}]},
        5
    ]);
    let values = bad.as_array().expect("array").clone();
    let problems = Filters::compile(&values, &schema, "channels[0].filters").expect_err("invalid");
    let text: Vec<String> = problems.iter().map(ToString::to_string).collect();
    insta::assert_snapshot!(text.join("\n"));
}

#[test]
fn overlong_patterns_and_deep_nesting_are_rejected() {
    let long = "a".repeat(201);
    let problems = Filter::compile(&cmp("PID.3", "matches", json!(long)), &Schema::hl7(), "f")
        .expect_err("too long");
    assert_eq!(problems[0].path, "f.value");
    let at_limit = "a".repeat(200);
    assert!(
        Filter::compile(
            &cmp("PID.3", "matches", json!(at_limit)),
            &Schema::hl7(),
            "f"
        )
        .is_ok()
    );

    let mut nested = json!({"path": "PID.3", "op": "exists"});
    for _ in 0..100 {
        nested = json!({ "not": nested });
    }
    let problems = Filter::compile(&nested, &Schema::hl7(), "f").expect_err("too deep");
    assert!(problems[0].message.contains("nested too deeply"));
}

fn arbitrary_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        "\\PC{0,8}".prop_map(Value::from),
        prop::sample::select(vec![
            "path",
            "op",
            "value",
            "all",
            "any",
            "not",
            "equals",
            "gt",
            "matches",
            "PID.3",
            "$.3",
            "ignore_case"
        ])
        .prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::hash_map(
                prop::sample::select(vec![
                    "path",
                    "op",
                    "value",
                    "all",
                    "any",
                    "not",
                    "ignore_case",
                    "x"
                ]),
                inner,
                0..5
            )
            .prop_map(|m| Value::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())),
        ]
    })
}

fn well_formed() -> impl Strategy<Value = Value> {
    let operators = prop::sample::select(vec![
        "equals",
        "not_equals",
        "in",
        "not_in",
        "starts_with",
        "contains",
        "matches",
        "exists",
        "not_exists",
        "gt",
        "gte",
        "lt",
        "lte",
    ]);
    let paths = prop::sample::select(vec![
        "MSH.9",
        "MSH.9.1",
        "PID.3",
        "PID.3[1].1",
        "PID.5",
        "OBX.5",
        "OBX[3].5",
        "ZZZ.1.1.1",
        "PID.99",
    ]);
    let values = prop_oneof![
        "\\PC{0,10}".prop_map(Value::from),
        any::<i32>().prop_map(Value::from),
        prop::collection::vec("\\PC{0,5}".prop_map(Value::from), 1..4).prop_map(Value::Array),
    ];
    let leaf = (paths, operators, values, any::<bool>()).prop_map(
        |(path, op, value, ic)| json!({"path": path, "op": op, "value": value, "ignore_case": ic}),
    );
    leaf.prop_recursive(5, 40, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..4).prop_map(|v| json!({"all": v})),
            prop::collection::vec(inner.clone(), 1..4).prop_map(|v| json!({"any": v})),
            inner.prop_map(|v| json!({"not": v})),
        ]
    })
}

proptest! {
    #[test]
    fn compiling_arbitrary_json_never_panics(value in arbitrary_json()) {
        let _ = Filter::compile(&value, &Schema::hl7(), "f");
        let _ = Filter::compile(&value, &Schema::astm(), "f");
    }

    #[test]
    fn evaluating_compiled_filters_never_panics(expression in well_formed(), text in "\\PC{0,40}") {
        if let Ok(filter) = Filter::compile(&expression, &Schema::hl7(), "f") {
            let mut message = oru();
            message.set_text("PID.5", &text);
            message.set_text("OBX.5", &text);
            let _ = filter.matches(&message);
            let _ = filter.matches(&Hl7Model::new());
            let _ = filter.matches(&RecordModel::new());
        }
    }

    #[test]
    fn numeric_filters_agree_with_integer_comparison(a in any::<i64>(), b in any::<i64>()) {
        let mut message = Hl7Model::new();
        message.set_text("OBX.5", &a.to_string());
        for (op, expected) in [("gt", a > b), ("gte", a >= b), ("lt", a < b), ("lte", a <= b)] {
            let filter = Filter::compile(&cmp("OBX.5", op, json!(b.to_string())), &Schema::hl7(), "f")
                .expect("valid");
            prop_assert_eq!(filter.matches(&message), expected, "{} {} {}", a, op, b);
        }
    }
}

trait SetText {
    fn set_text(&mut self, path: &str, text: &str);
}

impl SetText for Hl7Model {
    fn set_text(&mut self, path: &str, text: &str) {
        let path =
            packie_transform::Path::parse(packie_transform::Syntax::Hl7, path).expect("valid path");
        // Write to the first subcomponent so arbitrary text is stored literally.
        let leaf = format!("{path}.1.1");
        let leaf = packie_transform::Path::parse(packie_transform::Syntax::Hl7, &leaf)
            .expect("valid path");
        self.set(&leaf, text).expect("write");
    }
}
