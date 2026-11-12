//! The shipped instrument profiles: every connector patch and mapping preset validates
//! inside a complete document, and every mapping reproduces its sample byte for byte.
#![cfg(test)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::HashSet;
use std::path::Path;

use packie_config::profiles::{Catalog, ConnectorRole, MappingProfile, catalog};
use packie_config::{RuntimeConfig, load_runtime_file};
use packie_transform::{AstmModel, Hl7Model, Mapping, Model, Path as FieldPath, Schema, Syntax};
use serde_json::{Value, json};

fn shipped() -> Catalog {
    catalog().expect("the shipped catalog parses")
}

fn base() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/runtime.json");
    serde_json::to_value(load_runtime_file(&path).unwrap()).unwrap()
}

fn problems(document: &Value) -> Vec<String> {
    match RuntimeConfig::from_value(document.clone()) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.problems().iter().map(ToString::to_string).collect(),
    }
}

/// The sample document's message channel with this source and destination.
fn channel(source: (&str, Value), destination: (&str, Value), mapping: Value) -> Value {
    let mut document = base();
    document["channels"][1]["source"]["protocol"] = json!(source.0);
    document["channels"][1]["source"]["settings"] = source.1;
    let destination_endpoint = &mut document["channels"][1]["destinations"][0];
    destination_endpoint["protocol"] = json!(destination.0);
    destination_endpoint["settings"] = destination.1;
    destination_endpoint["mapping"] = mapping;
    // the protocol decides what an acknowledgement can prove
    destination_endpoint["delivery"]["ack_required"] = Value::Null;
    document
}

fn astm_source() -> (&'static str, Value) {
    ("astm", json!({}))
}

#[test]
fn the_catalog_has_unique_ids_and_the_documented_format() {
    let catalog = shipped();
    assert_eq!(catalog.format, 1);
    let mut ids = HashSet::new();
    for id in catalog
        .connectors
        .iter()
        .map(|p| &p.id)
        .chain(catalog.mappings.iter().map(|p| &p.id))
    {
        assert!(ids.insert(id.clone()), "duplicate id {id}");
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{id}"
        );
    }
    assert!(!catalog.connectors.is_empty() && !catalog.mappings.is_empty());
}

#[test]
fn every_connector_profile_is_a_valid_patch_for_each_of_its_roles() {
    for profile in &shipped().connectors {
        assert!(!profile.roles.is_empty(), "{}", profile.id);
        let settings = profile.settings.clone();
        assert!(settings.as_object().is_some_and(|o| !o.is_empty()));
        for role in &profile.roles {
            let document = match (profile.protocol.as_str(), role) {
                (protocol, ConnectorRole::Source) => channel(
                    (protocol, settings.clone()),
                    (protocol, settings.clone()),
                    Value::Null,
                ),
                (protocol, ConnectorRole::Destination) => channel(
                    astm_or_hl7_source(protocol),
                    (protocol, settings.clone()),
                    Value::Null,
                ),
            };
            let found = problems(&document);
            assert!(found.is_empty(), "{} as {role:?}: {found:#?}", profile.id);
        }
    }
}

/// A source of the same protocol, so a pass-through destination is valid.
fn astm_or_hl7_source(protocol: &str) -> (&'static str, Value) {
    match protocol {
        "hl7_mllp" => ("hl7_mllp", json!({})),
        _ => astm_source(),
    }
}

#[test]
fn a_connector_profile_changes_only_what_it_lists() {
    for profile in &shipped().connectors {
        let keys: Vec<&String> = profile.settings.as_object().unwrap().keys().collect();
        for key in keys {
            // every key is a real setting: unknown keys are validation errors
            let document = channel(
                astm_or_hl7_source(&profile.protocol),
                (
                    profile.protocol.as_str(),
                    json!({ key: profile.settings[key] }),
                ),
                Value::Null,
            );
            let found = problems(&document);
            assert!(found.is_empty(), "{} {key}: {found:#?}", profile.id);
        }
    }
}

fn model_of(protocol: &str, input: &str) -> Box<dyn packie_transform::Model> {
    match protocol {
        "astm" => Box::new(AstmModel::parse(input.as_bytes()).expect("sample parses as ASTM")),
        "hl7_mllp" => Box::new(Hl7Model::parse(input.as_bytes()).expect("sample parses as HL7")),
        other => panic!("sample of {other} is checked elsewhere"),
    }
}

fn schema_of(protocol: &str) -> Schema {
    match protocol {
        "astm" => Schema::astm(),
        "hl7_mllp" => Schema::hl7(),
        "au" => Schema::au(),
        other => panic!("unknown protocol {other}"),
    }
}

/// Runs `preset` on its sample; `None` for sources the transform tests cannot parse here.
fn run(preset: &MappingProfile) -> Option<String> {
    if preset.from == "au" {
        return None;
    }
    Some(run_on(preset, &preset.sample.input).unwrap_or_else(|e| panic!("{}: {e}", preset.id)))
}

/// Runs `preset` on `input`; the transform error text when it refuses.
fn run_on(preset: &MappingProfile, input: &str) -> Result<String, String> {
    let mapping = Mapping::compile(
        &preset.mapping,
        &schema_of(&preset.from),
        &schema_of(&preset.to),
        "mapping",
    )
    .unwrap_or_else(|problems| panic!("{} should compile: {problems:?}", preset.id));
    let source = model_of(&preset.from, input);
    match preset.to.as_str() {
        "hl7_mllp" => {
            let mut target = Hl7Model::new();
            for (path, value) in &preset.sample.header {
                let path = FieldPath::parse(Syntax::Hl7, path).expect("header path");
                target.set(&path, value).expect("header field");
            }
            mapping
                .apply(source.as_ref(), &mut target)
                .map_err(|e| e.to_string())?;
            Ok(String::from_utf8(target.encode()).expect("text"))
        }
        "astm" => {
            let mut target = AstmModel::new();
            mapping
                .apply(source.as_ref(), &mut target)
                .map_err(|e| e.to_string())?;
            Ok(String::from_utf8(target.encode()).expect("text"))
        }
        other => panic!("unknown destination {other}"),
    }
}

#[test]
fn every_mapping_preset_reproduces_its_sample() {
    let mut wrong = Vec::new();
    for preset in &shipped().mappings {
        let Some(output) = run(preset) else {
            continue;
        };
        if output != preset.sample.output {
            wrong.push(format!(
                "{}\n  got  {output:?}\n  want {:?}",
                preset.id, preset.sample.output
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn an_unedited_preset_does_not_fail_on_unlisted_test_codes() {
    // The tests table ships empty and a lookup into it keeps or skips what it does not
    // know: no lookup of a shipped preset is a hidden failure.
    for preset in &shipped().mappings {
        let tables = preset.mapping["tables"].as_object().unwrap();
        for name in ["tests", "flags"] {
            if let Some(table) = tables.get(name) {
                assert!(
                    table.as_object().unwrap().is_empty(),
                    "{} {name}",
                    preset.id
                );
            }
        }
        let mut lookups = Vec::new();
        collect_lookups(&preset.mapping["rules"], &mut lookups);
        for (table, on_missing) in lookups {
            assert_ne!(on_missing, "error", "{} {table}", preset.id);
        }
    }
}

fn collect_lookups(rules: &Value, found: &mut Vec<(String, String)>) {
    for rule in rules.as_array().into_iter().flatten() {
        if rule["op"] == "lookup" {
            found.push((
                rule["table"].as_str().unwrap().to_owned(),
                rule["on_missing"].as_str().unwrap_or("error").to_owned(),
            ));
        }
        for nested in ["rules", "then", "else"] {
            collect_lookups(&rule[nested], found);
        }
    }
}

#[test]
fn every_mapping_preset_is_valid_in_a_complete_document() {
    for preset in &shipped().mappings {
        let source_settings = preset
            .sample
            .source_settings
            .clone()
            .unwrap_or_else(|| json!({}));
        let destination_settings = preset
            .destination_settings
            .clone()
            .unwrap_or_else(|| json!({}));
        let document = channel(
            (preset.from.as_str(), source_settings),
            (preset.to.as_str(), destination_settings),
            preset.mapping.clone(),
        );
        let found = problems(&document);
        assert!(found.is_empty(), "{}: {found:#?}", preset.id);
    }
}

fn preset(id: &str) -> MappingProfile {
    shipped().mapping(id).expect("shipped preset").clone()
}

#[test]
fn result_presets_refuse_a_batch_instead_of_mixing_specimens() {
    for id in [
        "astm_results_to_hl7_oru",
        "astm_results_to_hl7_oru_roche_cobas",
        "astm_results_to_hl7_oru_instrument_specimen_id",
    ] {
        let preset = preset(id);
        let input = &preset.sample.input;
        // a second order (and its results) after the first
        let order_at = input.find("O|1|").expect("an order record");
        let end = input.find("L|1|").expect("the terminator");
        let batch = format!(
            "{}{}{}",
            &input[..end],
            &input[order_at..end],
            &input[end..]
        );
        let refused = run_on(&preset, &batch).expect_err(id);
        assert!(refused.contains("more than one"), "{id}: {refused}");
        assert!(run_on(&preset, input).is_ok(), "{id}");
    }
}

/// An ORM^O01 order of `tests` OBR, built from the preset's sample header lines.
fn orm(tests: usize) -> String {
    let mut text = String::from(
        "MSH|^~\\&|LIS|HOSP|PACKIE|LAB|20261012120000||ORM^O01|ORD-1|P|2.5.1\rPID|1||PRAC-1||DOE^JANE\r",
    );
    for n in 1..=tests {
        text.push_str(&format!(
            "ORC|NW|PL-{n}\rOBR|{n}|PL-{n}|SPEC1234|T{n}^Test{n}^L|R|||20261012110000\r"
        ));
    }
    text
}

const ORM_DOWNLOADS: [&str; 2] = [
    "hl7_orm_to_astm_download",
    "hl7_orm_to_astm_download_roche_cobas",
];

#[test]
fn a_download_carries_18_tests_in_one_order_record_and_refuses_19() {
    for id in ORM_DOWNLOADS {
        let preset = preset(id);
        let sent = run_on(&preset, &orm(18)).expect(id);
        let order = sent
            .split('\r')
            .find(|l| l.starts_with("O|"))
            .expect("one order record");
        assert_eq!(sent.matches("O|").count(), 1, "{id}: one order record");
        assert_eq!(order.matches("\\").count(), 17, "{id}: 18 repetitions");
        assert!(order.contains("^^^T1\\^^^T2\\"), "{id}: {order}");
        assert!(order.contains("^^^T18"), "{id}: the 18th test is sent");

        let refused = run_on(&preset, &orm(19)).expect_err(id);
        assert!(
            refused.contains("require[") && refused.contains("OBR"),
            "{id}: {refused}"
        );
        assert!(
            refused.contains("19") && refused.contains("18"),
            "{id}: {refused}"
        );
        assert!(
            run_on(&preset, &orm(0)).is_err(),
            "{id}: an order needs a test"
        );
    }
}

/// `input` with the first `from` replaced by `to`; the case is only meaningful if it changed.
fn changed(input: &str, from: &str, to: &str) -> String {
    assert!(input.contains(from), "the sample holds {from:?}");
    input.replacen(from, to, 1)
}

#[test]
fn a_download_refuses_what_is_not_one_patient_one_specimen_one_order() {
    let orm_cases = |input: String| {
        vec![
            (
                "two patients",
                changed(&input, "ORC|NW|PL-2", "PID|2||OTHER\rORC|NW|PL-2"),
            ),
            (
                "mixed order controls",
                changed(&input, "ORC|NW|PL-2", "ORC|CA|PL-2"),
            ),
            (
                "mixed priorities",
                changed(&input, "T2^Test2^L|R", "T2^Test2^L|S"),
            ),
            (
                "mixed specimen ids",
                changed(&input, "PL-2|SPEC1234", "PL-2|SPEC9999"),
            ),
        ]
    };
    for id in ORM_DOWNLOADS {
        let preset = preset(id);
        for (what, input) in orm_cases(orm(2)) {
            let refused = run_on(&preset, &input).expect_err(&format!("{id}: {what}"));
            assert!(refused.contains("require["), "{id} {what}: {refused}");
        }
        assert!(
            run_on(&preset, &orm(2)).is_ok(),
            "{id}: the unchanged order is accepted"
        );
    }

    let preset = preset("hl7_oml_o33_to_astm_download");
    let sample = preset.sample.input.clone();
    assert!(run_on(&preset, &sample).is_ok());
    let oml_cases = [
        (
            "two patients",
            changed(&sample, "ORC|NW|PL-2", "PID|2||OTHER\rORC|NW|PL-2"),
        ),
        (
            "two specimens",
            changed(&sample, "ORC|NW|PL-2", "SPM|2|SPEC9999||SER\rORC|NW|PL-2"),
        ),
        ("no specimen", changed(&sample, "SPM|1|SPEC1234||SER\r", "")),
        (
            "mixed order controls",
            changed(&sample, "ORC|NW|PL-2", "ORC|CA|PL-2"),
        ),
        (
            "mixed priorities",
            changed(&sample, "TSH^TSH^L|R", "TSH^TSH^L|S"),
        ),
    ];
    for (what, input) in oml_cases {
        let refused = run_on(&preset, &input).expect_err(what);
        assert!(refused.contains("require["), "{what}: {refused}");
    }
}

#[test]
fn an_oml_specimen_id_with_a_namespace_is_sent_as_the_bare_identifier() {
    let preset = preset("hl7_oml_o33_to_astm_download");
    let input = changed(&preset.sample.input, "SPM|1|SPEC1234", "SPM|1|SPEC1234&LIS");
    let sent = run_on(&preset, &input).expect("accepted");
    assert!(sent.contains("O|1|SPEC1234|"), "{sent:?}");
}
