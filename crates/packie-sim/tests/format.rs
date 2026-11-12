//! The scenario format: every shipped scenario is valid, and each rule of the format
//! rejects what it should, at load, with the location of the problem.
#![cfg(test)]

use std::path::Path;

use packie_sim::scenario::Scenario;

fn parse(text: &str) -> Result<Scenario, String> {
    Scenario::parse(text, Path::new("demo.json")).map_err(|e| e.to_string())
}

/// A valid scenario with `steps` and `actors` spliced in.
fn doc(extra: &str, actors: &str, steps: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "demo", "description": "d", "requirements": [], "timeout_ms": 1000,
            "resources": {{"ports": ["a"], "folders": ["out"]}},
            "engine": {{"runtime": {{}}}},
            "actors": {{{actors}}},
            "steps": [{steps}] {extra}}}"#
    )
}

fn actors() -> &'static str {
    r#""srv": {"kind": "tcp_server"}, "cli": {"kind": "tcp_client"}, "files": {"kind": "folder", "resource": "out"}"#
}

#[test]
fn all_shipped_scenarios_are_valid() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/scenarios");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            Scenario::load(&path).unwrap_or_else(|e| panic!("{e}"));
            count += 1;
        }
    }
    assert_eq!(count, 30, "the scenario suite has 30 files");
}

#[test]
fn a_minimal_scenario_is_valid() {
    parse(&doc(
        "",
        actors(),
        r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
    ))
    .unwrap();
}

#[test]
fn unknown_fields_are_errors_at_every_level() {
    let top = parse(&doc(
        r#", "colour": 1"#,
        actors(),
        r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
    ));
    assert!(top.unwrap_err().contains("colour"));
    let step = parse(&doc(
        "",
        actors(),
        r#"{"do": "wait", "actor": "engine", "ms": 1, "mss": 2}"#,
    ))
    .unwrap_err();
    assert!(step.contains("steps[0]") && step.contains("mss"), "{step}");
    let actor = parse(&doc(
        "",
        r#""srv": {"kind": "tcp_server", "bnd": "x"}"#,
        r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
    ))
    .unwrap_err();
    assert!(
        actor.contains("actors.srv") && actor.contains("bnd"),
        "{actor}"
    );
}

#[test]
fn unknown_steps_actors_and_placeholders_fail_at_load() {
    let step = parse(&doc("", actors(), r#"{"do": "teleport", "actor": "srv"}"#)).unwrap_err();
    assert!(step.contains("teleport"), "{step}");
    let actor = parse(&doc("", actors(), r#"{"do": "accept", "actor": "ghost"}"#)).unwrap_err();
    assert!(actor.contains("unknown actor `ghost`"), "{actor}");
    let placeholder = parse(&doc(
        "",
        actors(),
        r#"{"do": "connect", "actor": "cli", "host": "x", "port": "${port.zzz}"}"#,
    ))
    .unwrap_err();
    assert!(
        placeholder.contains("steps[0]") && placeholder.contains("port.zzz"),
        "{placeholder}"
    );
}

#[test]
fn steps_must_fit_their_actor() {
    let wrong_kind = parse(&doc("", actors(), r#"{"do": "accept", "actor": "cli"}"#)).unwrap_err();
    assert!(
        wrong_kind.contains("cannot run on actor `cli`"),
        "{wrong_kind}"
    );
    let engine_step = parse(&doc("", actors(), r#"{"do": "assert_snapshot", "actor": "srv", "target": {"node": true}, "expect": {"a": 1}}"#)).unwrap_err();
    assert!(engine_step.contains("engine step"), "{engine_step}");
    let folder_step = parse(&doc(
        "",
        actors(),
        r#"{"do": "write_file", "actor": "srv", "name": "x", "text": "y"}"#,
    ))
    .unwrap_err();
    assert!(
        folder_step.contains("cannot run on actor `srv`"),
        "{folder_step}"
    );
}

#[test]
fn payload_forms_are_exclusive_and_well_formed() {
    let none = parse(&doc("", actors(), r#"{"do": "send", "actor": "cli"}"#)).unwrap_err();
    assert!(none.contains("exactly one"), "{none}");
    let two = parse(&doc(
        "",
        actors(),
        r#"{"do": "send", "actor": "cli", "text": "a", "hex": "00"}"#,
    ))
    .unwrap_err();
    assert!(two.contains("exactly one"), "{two}");
    let bad_hex = parse(&doc(
        "",
        actors(),
        r#"{"do": "expect", "actor": "srv", "hex": "zz"}"#,
    ))
    .unwrap_err();
    assert!(bad_hex.contains("hex"), "{bad_hex}");
}

#[test]
fn capabilities_must_be_declared() {
    let hooks = parse(&doc(
        "",
        actors(),
        r#"{"do": "crash_engine", "actor": "engine"}"#,
    ))
    .unwrap_err();
    assert!(hooks.contains("test_hooks"), "{hooks}");
    let ok = parse(&doc(
        r#", "requires": ["test_hooks"]"#,
        actors(),
        r#"{"do": "crash_engine", "actor": "engine"}"#,
    ));
    assert!(ok.is_ok(), "{ok:?}");
    let serial = parse(
        r#"{"format": 1, "name": "demo", "description": "d", "requirements": [], "timeout_ms": 1000,
            "resources": {"serial_pairs": ["p"]}, "engine": {"runtime": {}},
            "actors": {"s": {"kind": "serial_peer", "pair": "p"}},
            "steps": [{"do": "connect", "actor": "s"}]}"#,
    )
    .unwrap_err();
    assert!(serial.contains("serial_pair"), "{serial}");
}

#[test]
fn name_must_match_the_file_and_the_pattern() {
    let text = doc(
        "",
        actors(),
        r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
    );
    let err = Scenario::parse(&text, Path::new("other.json"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("must equal the file name"), "{err}");
    let err = parse(&text.replace(r#""name": "demo""#, r#""name": "Demo_1""#)).unwrap_err();
    assert!(err.contains("^[a-z0-9-]+$"), "{err}");
}

#[test]
fn async_operations_need_names_and_awaits_need_a_prior_async_step() {
    let unnamed = parse(&doc(
        "",
        actors(),
        r#"{"do": "send", "actor": "cli", "text": "x", "async": true}"#,
    ))
    .unwrap_err();
    assert!(unnamed.contains("needs a `name`"), "{unnamed}");
    let missing = parse(&doc(
        "",
        actors(),
        r#"{"do": "await_send", "actor": "cli", "name": "w"}"#,
    ))
    .unwrap_err();
    assert!(missing.contains("no earlier async step"), "{missing}");
    let ok = parse(&doc(
        "",
        actors(),
        r#"{"do": "send", "actor": "cli", "text": "x", "async": true, "name": "w"}, {"do": "await_send", "actor": "cli", "name": "w"}"#,
    ));
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn engine_start_and_stop_must_be_in_order() {
    let twice = parse(&doc(
        "",
        actors(),
        r#"{"do": "start_engine", "actor": "engine"}"#,
    ))
    .unwrap_err();
    assert!(twice.contains("already running"), "{twice}");
    let stopped = parse(&doc(
        "",
        actors(),
        r#"{"do": "stop_engine", "actor": "engine"}, {"do": "assert_snapshot", "actor": "engine", "target": {"node": true}, "expect": {"a": 1}}"#,
    ))
    .unwrap_err();
    assert!(stopped.contains("stopped"), "{stopped}");
}

#[test]
fn matchers_and_paths_are_checked_at_load() {
    let bad_op = parse(&doc("", actors(), r#"{"do": "assert_snapshot", "actor": "engine", "target": {"node": true}, "expect": {"a": {"nope": 1}}}"#)).unwrap_err();
    assert!(bad_op.contains("nope"), "{bad_op}");
    let bad_path = parse(&doc("", actors(), r#"{"do": "assert_snapshot", "actor": "engine", "target": {"node": true}, "expect": {"a..b": 1}}"#)).unwrap_err();
    assert!(bad_path.contains("empty segment"), "{bad_path}");
}

#[test]
fn exactly_one_runtime_source_is_required() {
    let none = parse(
        &doc(
            "",
            actors(),
            r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
        )
        .replace(r#""runtime": {}"#, r#""start": "auto""#),
    )
    .unwrap_err();
    assert!(
        none.contains("exactly one of `runtime` and `runtime_file`"),
        "{none}"
    );
}
