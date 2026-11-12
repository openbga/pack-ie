//! Behavior of the configuration documents: samples, scenarios, validation problems,
//! store, diff.
#![cfg(test)]

use std::path::Path;
use std::time::Duration;

use packie_config::{
    Bootstrap, Change, ChannelConfig, ConfigDiff, ConfigError, ConfigStore, ProblemCode,
    ProblemSeverity, RuntimeConfig, Transport, load_runtime_file,
};
use serde_json::{Value, json};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn sample() -> RuntimeConfig {
    load_runtime_file(&repo().join("config/runtime.json")).unwrap()
}

fn sample_value() -> Value {
    serde_json::to_value(sample()).unwrap()
}

/// `path: message` of every problem, or nothing when the document is valid.
fn problems(value: &Value) -> Vec<String> {
    match RuntimeConfig::from_value(value.clone()) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.problems().iter().map(ToString::to_string).collect(),
    }
}

/// `(code, path)` of every problem.
fn codes(value: &Value) -> Vec<(ProblemCode, String)> {
    match RuntimeConfig::from_value(value.clone()) {
        Ok(_) => Vec::new(),
        Err(errors) => errors
            .into_problems()
            .into_iter()
            .map(|p| (p.code, p.path))
            .collect(),
    }
}

fn assert_has(found: &[String], expected: &[&str]) {
    for prefix in expected {
        assert!(
            found.iter().any(|e| e.starts_with(prefix)),
            "missing {prefix:?} in {found:#?}"
        );
    }
    assert_eq!(found.len(), expected.len(), "{found:#?}");
}

#[test]
fn sample_documents_are_valid() {
    let text = std::fs::read_to_string(repo().join("config/runtime.json")).unwrap();
    let (runtime, warnings) = RuntimeConfig::parse_with_warnings(&text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!((runtime.endpoints.len(), runtime.channels.len()), (4, 2));

    let bootstrap = Bootstrap::load(&repo().join("config/bootstrap.json")).unwrap();
    assert_eq!(bootstrap.node.node_id, "dev-node");
    assert!(bootstrap.state_dir().is_absolute());
    assert!(!bootstrap.state_dir().to_string_lossy().contains(".."));
    assert!(bootstrap.runtime_file().ends_with("state/runtime.json"));
    if cfg!(unix) {
        assert!(bootstrap.ipc_endpoint().ends_with("state/packie.sock"));
    }
}

#[test]
fn recent_errors_kept_follow_the_document_else_the_bootstrap() {
    let bootstrap = Bootstrap::load(&repo().join("config/bootstrap.json")).unwrap();
    let mut runtime = RuntimeConfig::empty();
    assert_eq!(
        runtime.retain_recent_errors(&bootstrap),
        bootstrap.diagnostics.retain_recent_errors as usize
    );
    runtime.diagnostics.retain_recent_errors = Some(25);
    assert_eq!(runtime.retain_recent_errors(&bootstrap), 25);
}

/// Replaces scenario placeholders the way the runner does: a string that is exactly one
/// numeric placeholder becomes a number (a distinct port per placeholder).
fn substitute(value: &mut Value, ports: &mut std::collections::HashMap<String, u16>) {
    match value {
        Value::String(text) => {
            let numeric = text.starts_with("${port.")
                || (text.starts_with("${actor.") && text.ends_with(".port}"));
            if numeric && text.matches("${").count() == 1 && text.ends_with('}') {
                let next = 40_000 + u16::try_from(ports.len()).unwrap();
                *value = json!(*ports.entry(text.clone()).or_insert(next));
                return;
            }
            let mut out = text.clone();
            while let Some(start) = out.find("${") {
                let Some(len) = out[start..].find('}') else {
                    break;
                };
                let key = out[start + 2..start + len].to_owned();
                let replacement = if key.ends_with(".host") {
                    "127.0.0.1".to_owned()
                } else {
                    format!("/tmp/packie-{}", key.replace('.', "-"))
                };
                out.replace_range(start..=start + len, &replacement);
            }
            *text = out;
        }
        Value::Array(items) => items.iter_mut().for_each(|v| substitute(v, ports)),
        Value::Object(map) => map.values_mut().for_each(|v| substitute(v, ports)),
        _ => {}
    }
}

#[test]
fn every_scenario_runtime_validates() {
    let mut checked = 0;
    for entry in std::fs::read_dir(repo().join("tests/scenarios")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let scenario: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let Some(runtime) = scenario["engine"].get("runtime") else {
            continue;
        };
        let mut runtime = runtime.clone();
        substitute(&mut runtime, &mut std::collections::HashMap::new());
        let found = problems(&runtime);
        assert!(found.is_empty(), "{}: {found:#?}", path.display());
        checked += 1;
    }
    assert!(checked >= 15, "only {checked} scenarios checked");
}

#[test]
fn serialized_form_round_trips_in_the_spec_shape() {
    let runtime = sample();
    let text = runtime.to_pretty_json().unwrap();
    assert_eq!(RuntimeConfig::parse(&text).unwrap(), runtime);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["endpoints"][0]["endpoint_id"], "analyzer-serial");
    assert_eq!(value["endpoints"][0]["kind"], "serial");
    assert_eq!(value["endpoints"][0]["settings"]["baud_rate"], 9600);
    assert_eq!(
        value["channels"][1]["destinations"][0]["protocol"],
        "hl7_mllp"
    );
    assert_eq!(value["channels"][0]["retry"]["multiplier"], 2.0);
}

#[test]
fn schema_errors_report_the_field_path() {
    let mut value = sample_value();
    value["channels"][0]["connection"]["stratgy"] = json!("parallel");
    let found = problems(&value);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains("Fix this and validate again"),
        "{found:?}"
    );
    assert!(
        found[0].starts_with("channels[0].connection.stratgy: unknown field `stratgy`"),
        "{found:?}"
    );

    let mut value = sample_value();
    value["endpoints"][2]["settings"]["allowed_peer_cidrs"] = json!(["10.0.0.0/40"]);
    assert_eq!(
        codes(&value),
        [(
            ProblemCode::InvalidCidr,
            "endpoints[2].settings.allowed_peer_cidrs[0]".to_owned()
        )]
    );

    let mut value = sample_value();
    value["channels"][1]["destinations"][0]["settings"]["ack_mod"] = json!("x");
    let found = problems(&value);
    assert!(
        found[0].starts_with("channels[1].destinations[0].settings.ack_mod: unknown field"),
        "{found:?}"
    );

    let mut value = sample_value();
    value["endpoints"][1]["endpoint_id"] = json!("Bad Id");
    assert!(problems(&value)[0].starts_with("endpoints[1].endpoint_id: not a valid id"));

    let errors = RuntimeConfig::parse("{ not json")
        .unwrap_err()
        .into_problems();
    assert!(errors[0].message.contains("line 1"), "{errors:?}");
}

#[test]
fn validation_reports_every_rule_with_codes() {
    let mut value = sample_value();
    value["endpoints"][1]["kind"] = json!("serial");
    value["endpoints"][1]["settings"] = json!({"port_name": "com3", "baud_rate": 9600});
    value["endpoints"][2]["settings"]["allowed_peer_cidrs"] = json!([]);
    value["endpoints"][3]["settings"]["port"] = json!(0);
    value["channels"][0]["endpoint_b"] = json!("analyzer-serial");
    value["channels"][0]["connection"] = json!({"connect_strategy": "receiver_then_sender"});
    value["channels"][0]["retry"] = json!({"initial_delay_ms": 5000, "max_delay_ms": 1000});
    value["channels"][0]["tap"] = json!({"framer": "none", "record_messages": true});
    value["channels"][1]["source"]["endpoint"] = json!("missing");
    value["channels"][1]["destinations"][0]["delivery"] = json!({"max_attempts": 0});
    value["alerts"] = json!({"buffer_high_percent": 0});
    let found: Vec<(ProblemCode, String)> = codes(&value);
    let expected = [
        (ProblemCode::Schema, "endpoints[3].settings.port"),
        (
            ProblemCode::AllowlistRequired,
            "endpoints[2].settings.allowed_peer_cidrs",
        ),
        (ProblemCode::PortConflict, "endpoints[1].settings.port_name"),
        (ProblemCode::SameEndpoint, "channels[0].endpoint_b"),
        (
            ProblemCode::ReceiverRequired,
            "channels[0].connection.receiver",
        ),
        (ProblemCode::RetryInvalid, "channels[0].retry.max_delay_ms"),
        (ProblemCode::TapInvalid, "channels[0].tap.record_messages"),
        (ProblemCode::UnknownEndpoint, "channels[1].source.endpoint"),
        (
            ProblemCode::Schema,
            "channels[1].destinations[0].delivery.max_attempts",
        ),
        (ProblemCode::Schema, "alerts.buffer_high_percent"),
    ];
    for (code, path) in expected {
        assert!(
            found.iter().any(|(c, p)| *c == code && p == path),
            "missing {code:?} {path} in {found:#?}"
        );
    }
    assert_eq!(found.len(), expected.len(), "{found:#?}");
}

#[test]
fn references_protocols_and_warnings() {
    let mut value = sample_value();
    value["endpoints"][3]["enabled"] = json!(false);
    value["channels"][1]["destinations"][0]["mapping"] = Value::Null;
    value["channels"][1]["destinations"][0]["delivery"] = json!({"ack_required": "accepted"});
    value["channels"][1]["destinations"][0]["settings"] = json!({"ack_mode": "none"});
    let found = codes(&value);
    let expected = [
        (
            ProblemCode::EndpointDisabled,
            "channels[1].destinations[0].endpoint",
        ),
        (
            ProblemCode::PassthroughProtocol,
            "channels[1].destinations[0].mapping",
        ),
        (
            ProblemCode::AckLevelUnreachable,
            "channels[1].destinations[0].delivery.ack_required",
        ),
    ];
    for (code, path) in expected {
        assert!(
            found.iter().any(|(c, p)| *c == code && p == path),
            "missing {code:?} {path} in {found:#?}"
        );
    }

    // Warnings do not reject.
    let mut value = sample_value();
    value["channels"][1]["retention"] = json!({"max_age_days": null});
    let copy = value["channels"][0].clone();
    let mut second = copy;
    second["channel_id"] = json!("analyzer-bridge-2");
    value["channels"].as_array_mut().unwrap().push(second);
    let text = serde_json::to_string(&value).unwrap();
    let (_, warnings) = RuntimeConfig::parse_with_warnings(&text).unwrap();
    let warning_codes: Vec<ProblemCode> = warnings.iter().map(|w| w.code).collect();
    assert!(
        warnings
            .iter()
            .all(|w| w.severity == ProblemSeverity::Warning)
    );
    assert!(
        warning_codes.contains(&ProblemCode::RetentionUnbounded),
        "{warnings:?}"
    );
    assert!(
        warning_codes.contains(&ProblemCode::SharedExclusiveEndpoint),
        "{warnings:?}"
    );
}

#[test]
fn filters_mappings_and_inline_secrets() {
    let mut value = sample_value();
    value["channels"][1]["filters"] = json!([{"path": "R.3", "op": "equals", "value": "GLU"}, {"bogus": 1},
            {"path": "PID.3", "op": "exists"}]);
    value["channels"][1]["destinations"][0]["mapping"] = json!({"rules": [{"to": "MSH.9"}]});
    value["endpoints"].as_array_mut().unwrap().push(json!({
        "endpoint_id": "partner", "label": "Partner SFTP", "kind": "sftp",
        "settings": {"host": "sftp.example.org", "username": "lab",
            "auth": {"method": "password", "password": "hunter2"},
            "host_key_fingerprint": "SHA256:abcdefghijklmnop", "remote_directory": "/in"},
    }));
    let found = codes(&value);
    assert_eq!(
        found,
        [
            (
                ProblemCode::SecretInline,
                "endpoints[4].settings.auth.password".to_owned()
            ),
            (
                ProblemCode::FilterInvalid,
                "channels[1].filters[1].bogus".to_owned()
            ),
            (
                ProblemCode::FilterInvalid,
                "channels[1].filters[1].path".to_owned()
            ),
            (
                ProblemCode::FilterInvalid,
                "channels[1].filters[1].op".to_owned()
            ),
            // An HL7 path on an ASTM source: checked against the source's model.
            (
                ProblemCode::FilterInvalid,
                "channels[1].filters[2].path".to_owned()
            ),
            (
                ProblemCode::MappingInvalid,
                "channels[1].destinations[0].mapping.rules[0].op".to_owned()
            ),
        ]
    );
    let text = format!("{:?}", RuntimeConfig::from_value(value).unwrap_err());
    assert!(!text.contains("hunter2"), "a literal secret leaked: {text}");
}

#[test]
fn protocol_settings_follow_the_spec_ranges() {
    let mut value = sample_value();
    value["channels"][1]["source"]["settings"] =
        json!({"role": "instrument", "max_frame_chars": 70000, "max_frame_retries": 21});
    value["channels"][1]["destinations"][0]["settings"] =
        json!({"version": "3.0", "sending_application": "x".repeat(181)});
    let at = "channels[1].destinations[0].settings";
    assert_has(
        &problems(&value),
        &[
            "channels[1].source.settings.max_frame_retries: must be from 0 to 20",
            "channels[1].source.settings.max_frame_chars: must be from 64 to 64000",
            &format!("{at}.version: must be 2.x or 2.x.y"),
            &format!("{at}.sending_application: must be at most 180"),
        ],
    );

    for bad in ["O.2", "P.3", "O.3.0", "O.4.3.1", "SPM.2", ""] {
        let mut value = sample_value();
        value["channels"][1]["source"]["settings"] = json!({ "sample_id_path": bad });
        assert_has(
            &problems(&value),
            &["channels[1].source.settings.sample_id_path: must be O.<field>"],
        );
    }
    let mut value = sample_value();
    value["channels"][1]["source"]["settings"] = json!({ "sample_id_path": "O.4.3" });
    assert!(problems(&value).is_empty());

    let mut value = sample_value();
    value["channels"][1]["source"]["protocol"] = json!("au");
    value["channels"][1]["source"]["settings"] = json!({
        "profile": "au640", "frame_timeout_ms": 150,
        "framing": {"start": [2], "end": [6], "bcc": true, "use_etb": true,
            "max_wire_len": 512, "class": "b"},
        "format": {"unit_no": false, "rack_digits": 4, "sample_id_digits": 10, "sex": false,
            "age": false, "month": false, "patient_info_digits": [0, 0, 0, 0, 0, 0],
            "result_digits": 6},
    });
    value["channels"][1]["destinations"][0]["mapping"] = json!({"rules": []});
    let at = "channels[1].source.settings";
    assert_has(
        &problems(&value),
        &[
            &format!("{at}.framing.end: must be 1 or 2 control bytes"),
            &format!("{at}.framing.use_etb: only allowed when end is [3]"),
            &format!("{at}.frame_timeout_ms: must be 100 to 9900 in steps of 100"),
        ],
    );

    let mut value = sample_value();
    value["channels"][1]["destinations"][0]["endpoint"] = json!("analyzer-serial");
    assert_eq!(
        codes(&value),
        [
            (
                ProblemCode::EndpointKindNotAllowed,
                "channels[1].destinations[0].protocol".to_owned()
            ),
            // The serial port is now also used by the bridge.
            (ProblemCode::SharedExclusiveEndpoint, "channels".to_owned()),
        ]
    );
}

#[test]
fn store_keeps_the_newest_versions_and_skips_invalid_ones() {
    let dir = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(dir.path().join("config"), Some(3), Duration::ZERO).unwrap();
    assert!(store.latest().unwrap().is_none());
    let mut config = sample();
    for version in 1..=5 {
        config.config_version = version;
        store
            .save(&config, &format!("version {version}"), &operator())
            .unwrap();
    }
    let versions: Vec<u64> = store
        .history()
        .unwrap()
        .iter()
        .map(|v| v.config_version)
        .collect();
    assert_eq!(versions, [5, 4, 3]);
    assert_eq!(
        store.history().unwrap()[0].note.as_deref(),
        Some("version 5")
    );
    assert!(!store.dir().join("runtime.v0000000001.meta.json").exists());
    assert_eq!(store.latest().unwrap().unwrap().config_version, 5);
    assert!(matches!(store.load(1), Err(ConfigError::UnknownVersion(1))));
    std::fs::write(
        store.dir().join("runtime.v0000000006.json"),
        "{\"schema_version\": 9}",
    )
    .unwrap();
    assert_eq!(store.latest().unwrap().unwrap().config_version, 5);
}

fn operator() -> packie_core::Actor {
    packie_core::Actor {
        name: "jdoe".into(),
        uid_or_sid: "501".into(),
        origin: packie_core::ActorOrigin::Console,
        pid: Some(4242),
    }
}

#[test]
fn a_version_is_never_active_without_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(dir.path().join("config"), None, Duration::ZERO).unwrap();
    let mut config = sample();
    for version in 1..=2 {
        config.config_version = version;
        store.save(&config, "routine change", &operator()).unwrap();
    }
    // A record that cannot be written (a directory in its place) fails the save before the
    // document is published.
    config.config_version = 3;
    std::fs::create_dir(store.dir().join("runtime.v0000000003.meta.json")).unwrap();
    assert!(store.save(&config, "routine change", &operator()).is_err());
    assert!(!store.dir().join("runtime.v0000000003.json").exists());
    // A document without its record (as an older engine could leave) is not loaded.
    config.config_version = 4;
    std::fs::write(
        store.dir().join("runtime.v0000000004.json"),
        config.to_pretty_json().unwrap(),
    )
    .unwrap();
    assert_eq!(store.latest().unwrap().unwrap().config_version, 2);
}

/// `runtime.versions_kept` absent or null keeps every version, even with no audit
/// protection period (acceptance N-05).
#[test]
fn null_versions_kept_keeps_every_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    for runtime in ["", r#","runtime":{"versions_kept":null}"#] {
        std::fs::write(
            &path,
            format!(r#"{{"node":{{"node_id":"n","display_name":"N"}},"paths":{{"state_directory":"s"}}{runtime}}}"#),
        )
        .unwrap();
        let bootstrap = Bootstrap::load(&path).unwrap();
        assert_eq!(bootstrap.runtime.versions_kept, None, "{runtime}");
    }
    let keep = None;
    let store = ConfigStore::open(dir.path().join("config"), keep, Duration::ZERO).unwrap();
    let mut config = sample();
    for version in 1..=60 {
        config.config_version = version;
        store.save(&config, "routine change", &operator()).unwrap();
    }
    let kept: Vec<u64> = store
        .history()
        .unwrap()
        .iter()
        .map(|v| v.config_version)
        .collect();
    assert_eq!(kept, (1..=60).rev().collect::<Vec<_>>());
}

#[test]
fn versions_record_who_applied_them_and_their_verifications() {
    let dir = tempfile::tempdir().unwrap();
    // No limit: every version stays.
    let store = ConfigStore::open(dir.path().join("config"), None, Duration::ZERO).unwrap();
    let mut config = sample();
    for version in 1..=30 {
        config.config_version = version;
        store.save(&config, "routine change", &operator()).unwrap();
    }
    let history = store.history().unwrap();
    assert_eq!(history.len(), 30);
    let newest = &history[0];
    assert_eq!(newest.applied_by.as_ref().unwrap().name, "jdoe");
    assert!(newest.applied_at.is_some() && newest.verifications.is_empty());

    let reviewer = packie_core::Actor {
        name: "director".into(),
        ..operator()
    };
    let verified = store.verify(30, &reviewer, "VAL-2026-017").unwrap();
    assert_eq!(verified.verifications.len(), 1);
    assert_eq!(verified.verifications[0].verified_by.name, "director");
    assert_eq!(verified.verifications[0].note, "VAL-2026-017");
    assert_eq!(store.history().unwrap()[0].verifications.len(), 1);
    let outcomes = [packie_config::ChannelOutcome {
        channel_id: "hema".parse().unwrap(),
        outcome: packie_config::ApplyOutcome::Restarted,
    }];
    store.record_outcomes(30, &outcomes).unwrap();
    let newest = &store.history().unwrap()[0];
    assert_eq!(newest.outcomes, outcomes);
    assert_eq!(newest.verifications.len(), 1, "outcomes keep the sign-offs");
    assert!(matches!(
        store.verify(99, &reviewer, "x"),
        Err(ConfigError::UnknownVersion(99))
    ));

    // A limit never removes versions younger than the protection period.
    let protected = ConfigStore::open(
        dir.path().join("config"),
        Some(2),
        Duration::from_secs(3600),
    )
    .unwrap();
    config.config_version = 31;
    protected.save(&config, "one more", &operator()).unwrap();
    assert_eq!(protected.history().unwrap().len(), 31);
}

#[cfg(unix)]
#[test]
fn saved_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(dir.path().join("config"), Some(3), Duration::ZERO).unwrap();
    store.save(&sample(), "first", &operator()).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&store.dir().join("runtime.v0000000000.json")), 0o600);
    assert_eq!(
        mode(&store.dir().join("runtime.v0000000000.meta.json")),
        0o600
    );
    assert_eq!(mode(store.dir()), 0o700);
}

#[cfg(unix)]
#[test]
fn atomic_writes_never_reuse_an_existing_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("secrets.json");
    // A stale, world-readable temp under the old fixed name, and a symlink planted under it
    // that points at a file the writer must not touch.
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "keep").unwrap();
    std::os::unix::fs::symlink(&victim, dir.path().join("secrets.json.tmp")).unwrap();
    let stale = dir.path().join(".secrets.json.tmp");
    std::fs::write(&stale, "stale").unwrap();
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();

    packie_config::write_atomic(&target, b"{\"sftp-password\": \"synthetic\"}").unwrap();
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(std::fs::read_to_string(&stale).unwrap(), "stale");
    let leftovers = std::fs::read_dir(dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count();
    assert_eq!(
        leftovers, 2,
        "only the planted temps remain; ours was renamed"
    );
}

#[test]
fn diff_separates_restarts_from_in_place_updates() {
    let old = sample();
    let mut new = old.clone();
    if let Transport::TcpListener(listener) = &mut new.endpoints[1].transport {
        listener.port = 5101;
    }
    if let ChannelConfig::Message(message) = &mut new.channels[1] {
        message.label = "Renamed".into();
        message.retention.max_age_days = Some(30);
    }
    let mut added = new.channels[0].clone();
    if let ChannelConfig::Bridge(bridge) = &mut added {
        bridge.id = "copy".parse().unwrap();
    }
    new.channels.push(added);

    let diff = ConfigDiff::between(&old, &new);
    let ids = |change| {
        diff.channels_with(change)
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(Change::Changed), ["analyzer-bridge"]);
    assert_eq!(ids(Change::Updated), ["analyzer-results"]);
    assert_eq!(ids(Change::Added), ["copy"]);
    assert_eq!(diff.endpoints[1].change, Change::Changed);

    let mut removed = old.clone();
    removed.channels.pop();
    assert_eq!(
        ConfigDiff::between(&old, &removed)
            .channels_with(Change::Removed)
            .count(),
        1
    );
    assert!(ConfigDiff::between(&old, &old.clone()).is_empty());
}

#[test]
fn channels_the_document_disables_are_stopped_in_the_plan() {
    let old = sample();
    let mut new = old.clone();
    if let ChannelConfig::Bridge(bridge) = &mut new.channels[0] {
        bridge.enabled = false;
    }
    let mut added = new.channels[0].clone();
    if let ChannelConfig::Bridge(bridge) = &mut added {
        bridge.id = "spare".parse().unwrap();
    }
    new.channels.push(added);
    let diff = ConfigDiff::between(&old, &new);
    let stopped: Vec<String> = diff
        .channels_with(Change::Stopped)
        .map(|id| id.to_string())
        .collect();
    assert_eq!(stopped, ["analyzer-bridge", "spare"]);
    let outcomes = packie_config::ApplyReport::planned(&diff);
    assert!(
        outcomes.channels[0].outcome == packie_config::ApplyOutcome::Stopped,
        "{outcomes:?}"
    );
}

/// Ten message channels with their own endpoints; a delivery-policy change to one of them
/// (as the soak's chaos applies) updates that one in place and leaves the others alone.
#[test]
fn a_policy_change_to_one_channel_leaves_the_other_nine_unchanged() {
    let mut value = sample_value();
    let template = value["channels"][1].clone();
    let (source, destination) = (
        template["source"]["endpoint"].clone(),
        template["destinations"][0]["endpoint"].clone(),
    );
    let endpoint = |id: &Value| -> Value {
        value["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["endpoint_id"] == *id)
            .unwrap()
            .clone()
    };
    let (source_endpoint, destination_endpoint) = (endpoint(&source), endpoint(&destination));
    let mut endpoints = Vec::new();
    let mut channels = Vec::new();
    for n in 1..=10 {
        let mut source = source_endpoint.clone();
        source["endpoint_id"] = json!(format!("in-{n:02}"));
        source["settings"]["port"] = json!(6000 + n);
        let mut destination = destination_endpoint.clone();
        destination["endpoint_id"] = json!(format!("out-{n:02}"));
        destination["settings"]["port"] = json!(7000 + n);
        let mut channel = template.clone();
        channel["channel_id"] = json!(format!("hl7-{n:02}"));
        channel["source"]["endpoint"] = source["endpoint_id"].clone();
        channel["destinations"][0]["endpoint"] = destination["endpoint_id"].clone();
        endpoints.extend([source, destination]);
        channels.push(channel);
    }
    value["endpoints"] = json!(endpoints);
    value["channels"] = json!(channels);
    let old = RuntimeConfig::from_value(value.clone()).unwrap();
    value["channels"][3]["destinations"][0]["delivery"] = json!({ "ack_timeout_seconds": 65 });
    let new = RuntimeConfig::from_value(value).unwrap();

    let outcomes = packie_config::ApplyReport::planned(&ConfigDiff::between(&old, &new));
    let changed: Vec<(String, packie_config::ApplyOutcome)> = outcomes
        .channels
        .into_iter()
        .filter(|c| c.outcome != packie_config::ApplyOutcome::Unchanged)
        .map(|c| (c.channel_id.to_string(), c.outcome))
        .collect();
    assert_eq!(
        changed,
        [("hl7-04".to_owned(), packie_config::ApplyOutcome::Updated)]
    );
}

#[test]
fn spelled_out_defaults_are_not_a_change() {
    let old = sample();
    let mut value = serde_json::to_value(&old).unwrap();
    value["channels"][0]
        .as_object_mut()
        .unwrap()
        .remove("buffer");
    let new = RuntimeConfig::from_value(value).unwrap();
    assert!(ConfigDiff::between(&old, &new).is_empty());
}

#[test]
fn bootstrap_audit_and_backup_settings_default_resolve_and_validate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    let write = |extra: &str| {
        std::fs::write(
            &path,
            format!(
                r#"{{"node":{{"node_id":"n","display_name":"N"}},"paths":{{"state_directory":"s"}}{extra}}}"#
            ),
        )
        .unwrap();
    };
    write("");
    let defaults = Bootstrap::load(&path).unwrap();
    assert_eq!(defaults.audit.retention_days, 2190);
    assert_eq!(defaults.backup.stale_limit_hours(), None);

    write(r#","backup":{"directory":"backups","schedule":{"daily_at":"02:30"}}"#);
    let scheduled = Bootstrap::load(&path).unwrap();
    let schedule = scheduled.backup.schedule.as_ref().unwrap();
    assert_eq!((schedule.time_of_day(), schedule.keep), (Some((2, 30)), 7));
    assert_eq!(scheduled.backup.stale_limit_hours(), Some(26));
    assert!(scheduled.backup.directory.as_ref().unwrap().is_absolute());

    write(
        r#","audit":{"retention_days":30},"backup":{"schedule":{"daily_at":"24:00","keep":0},"stale_after_hours":0}"#,
    );
    let Err(ConfigError::Invalid { errors, .. }) = Bootstrap::load(&path) else {
        panic!("accepted invalid audit and backup settings");
    };
    let paths: Vec<&str> = errors.problems().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "audit.retention_days",
            "backup.stale_after_hours",
            "backup.schedule.daily_at",
            "backup.schedule.keep",
            "backup.directory"
        ]
    );
}

#[test]
fn bootstrap_problems_have_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    std::fs::write(
        &path,
        r#"{"node":{"node_id":"Bad Id","display_name":""},"paths":{"state_directory":"s"},
            "logging":{"ring_buffer_events":10}}"#,
    )
    .unwrap();
    let Err(ConfigError::Invalid { errors, .. }) = Bootstrap::load(&path) else {
        panic!("accepted an invalid bootstrap");
    };
    let paths: Vec<&str> = errors.problems().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "node.node_id",
            "node.display_name",
            "logging.ring_buffer_events"
        ]
    );
    std::fs::write(&path, r#"{"node":{"node_id":"n","display_name":"N"}}"#).unwrap();
    let Err(ConfigError::Invalid { errors, .. }) = Bootstrap::load(&path) else {
        panic!("accepted a bootstrap without paths");
    };
    assert!(
        errors.problems()[0]
            .message
            .contains("missing field `paths`"),
        "{errors}"
    );
}

/// The secrets file is the default where no OS key store can be assumed (Linux), lives in the
/// state directory unless named, may be missing until the first secret is stored, and must
/// be owner-only once it exists.
#[test]
fn the_secrets_file_defaults_to_the_state_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    let write = |secrets: &str| {
        std::fs::write(
            &path,
            format!(r#"{{"node":{{"node_id":"n","display_name":"N"}},"paths":{{"state_directory":"s"}}{secrets}}}"#),
        )
        .unwrap();
    };
    write("");
    let default = Bootstrap::load(&path).unwrap().secrets;
    let expected = if cfg!(any(target_os = "macos", windows)) {
        packie_config::SecretsProvider::Keystore
    } else {
        packie_config::SecretsProvider::File
    };
    assert_eq!(default.provider, expected);

    // Where the key store is the default it is reachable (Keychain, Credential Manager).
    if expected == packie_config::SecretsProvider::Keystore {
        let bootstrap = Bootstrap::load(&path).unwrap();
        assert_eq!(packie_config::secret_store_problem(&bootstrap), None);
    }

    write(r#","secrets":{"provider":"file"}"#);
    let bootstrap = Bootstrap::load(&path).unwrap();
    assert_eq!(packie_config::secret_store_problem(&bootstrap), None);
    let file = bootstrap.secrets.file.unwrap();
    assert!(
        file.is_absolute() && file.ends_with("s/secrets.json"),
        "{}",
        file.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{}").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let Err(ConfigError::Invalid { errors, .. }) = Bootstrap::load(&path) else {
            panic!("accepted a secrets file other users can read");
        };
        assert_eq!(errors.problems()[0].path, "secrets.file");
    }
}

/// A client outside the service's group cannot reach the secrets file: the bootstrap still
/// loads (it only needs the management interface), while the engine refuses to start.
#[cfg(unix)]
#[test]
fn an_unreachable_secrets_file_stops_only_the_engine() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("s");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("secrets.json"), "{}").unwrap();
    std::fs::set_permissions(
        state.join("secrets.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let path = dir.path().join("bootstrap.json");
    std::fs::write(
        &path,
        r#"{"node":{"node_id":"n","display_name":"N"},"paths":{"state_directory":"s"},
            "secrets":{"provider":"file"}}"#,
    )
    .unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::metadata(state.join("secrets.json")).is_ok() {
        // Root reaches every file.
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let loaded = Bootstrap::load(&path);
    let problem = loaded
        .as_ref()
        .ok()
        .and_then(packie_config::secret_store_problem);
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let bootstrap = loaded.unwrap();
    assert!(bootstrap.secrets.file.is_some());
    let problem = problem.unwrap();
    assert!(
        problem.starts_with("secrets.file:") && problem.contains("ermission"),
        "{problem}"
    );
}

#[test]
fn bootstrap_notifications_are_off_by_default_and_validated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    let write = |extra: &str| {
        std::fs::write(
            &path,
            format!(
                r#"{{"node":{{"node_id":"n","display_name":"N"}},"paths":{{"state_directory":"s"}}{extra}}}"#
            ),
        )
        .unwrap();
    };
    write("");
    assert!(!Bootstrap::load(&path).unwrap().notifications.is_enabled());

    write(
        r#","notifications":{"webhook":{"url":"https://chat.lab.example/h","bearer":{"secret":"webhook-token"}}}"#,
    );
    let on = Bootstrap::load(&path).unwrap().notifications;
    assert!(on.is_enabled());
    assert_eq!(on.secret_refs()[0].0, "notifications.webhook.bearer");

    write(r#","notifications":{"ca_file":"certs/lab-ca.pem"}"#);
    let resolved = Bootstrap::load(&path)
        .unwrap()
        .notifications
        .ca_file
        .unwrap();
    assert!(resolved.is_absolute() && resolved.ends_with("certs/lab-ca.pem"));

    write(
        r#","notifications":{"rate_limit_minutes":0,"email":{"host":"mail.lab.example","port":25,"security":"none","from":"a@b.example","to":[]}}"#,
    );
    let Err(ConfigError::Invalid { errors, .. }) = Bootstrap::load(&path) else {
        panic!("accepted invalid notification settings");
    };
    let paths: Vec<&str> = errors.problems().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "notifications.rate_limit_minutes",
            "notifications.email.security",
            "notifications.email.to"
        ]
    );
}

#[test]
fn settings_summaries_never_show_secrets() {
    let summaries: Vec<String> = sample()
        .endpoints
        .iter()
        .map(|e| e.settings_summary())
        .collect();
    assert_eq!(
        summaries,
        [
            "COM3 9600 8N1",
            "192.168.10.10:5100 (1 allowed peer)",
            "192.168.10.10:5200 (1 allowed peer)",
            "192.168.10.20:6661",
        ]
    );
}

#[test]
fn file_and_virtual_serial_settings_are_validated() {
    let mut value = sample_value();
    value["endpoints"][0]["settings"]["virtual_link"] = json!("/tmp/ttyPACKIE0");
    value["endpoints"].as_array_mut().unwrap().push(json!({
        "endpoint_id": "drop", "label": "Drop folder", "kind": "folder",
        "settings": {"path": "/data/in", "processed_directory": "../out",
            "file_pattern": "", "poll_interval_seconds": 0},
    }));
    // Windows has no pseudo-terminals, so a virtual link is refused there outright.
    let virtual_link = if cfg!(windows) {
        "endpoints[0].settings.virtual_link: virtual serial links need Unix"
    } else {
        "endpoints[0].settings.virtual_link: a virtual link needs a path and port_name"
    };
    assert_has(
        &problems(&value),
        &[
            virtual_link,
            "endpoints[4].settings.file_pattern: must not be empty",
            "endpoints[4].settings.processed_directory: must be a directory inside",
            "endpoints[4].settings.poll_interval_seconds: must be from 1 to 86400",
        ],
    );

    // Backslashes separate too: the SFTP adapter and Windows read them as separators.
    for escape in [
        "..\\outside",
        "done\\..\\..\\x",
        "\\abs",
        "C:\\done",
        "/abs",
    ] {
        let mut value = sample_value();
        value["endpoints"].as_array_mut().unwrap().push(json!({
            "endpoint_id": "partner", "label": "Partner SFTP", "kind": "sftp",
            "settings": {"host": "sftp.example.org", "username": "lab",
                "auth": {"method": "password", "password": {"secret": "partner-pass"}},
                "host_key_fingerprint": "SHA256:abcdefghijklmnop", "remote_directory": "/in",
                "processed_directory": escape},
        }));
        assert_has(
            &problems(&value),
            &["endpoints[4].settings.processed_directory: must be a directory inside"],
        );
    }
}

#[test]
fn delimited_file_settings_are_validated() {
    let folder = |id: &str, path: &str| json!({"endpoint_id": id, "label": id, "kind": "folder", "settings": {"path": path}});
    let channel = |source: Value, destination: Value| {
        let mut value = sample_value();
        let endpoints = value["endpoints"].as_array_mut().unwrap();
        endpoints.push(folder("files-in", "/data/in"));
        endpoints.push(folder("files-out", "/data/out"));
        value["channels"].as_array_mut().unwrap().push(json!({
            "kind": "message", "channel_id": "files", "label": "Files",
            "source": {"endpoint": "files-in", "protocol": "delimited_file", "settings": source},
            "destinations": [{
                "destination_id": "out", "label": "Out", "endpoint": "files-out",
                "protocol": "delimited_file", "settings": destination,
            }],
        }));
        value
    };
    let columns = json!([{"name": "sample_id"}, {"name": "result", "type": "decimal"}]);
    let valid = |file_name: &str| {
        channel(
            json!({"columns": columns}),
            json!({"columns": columns, "file_name": file_name}),
        )
    };
    assert_eq!(problems(&valid("{message_id}.csv")), Vec::<String>::new());

    let at = "channels[2].destinations[0].settings.file_name";
    for bad in [".{message_id}.csv", "out/{message_id}.csv", " "] {
        assert_has(
            &problems(&valid(bad)),
            &[&format!("{at}: must be a file name")],
        );
    }

    let value = channel(
        json!({"columns": [{"name": "a"}, {"name": "a"}], "group_by": "a"}),
        json!({"columns": columns, "file_name": "x.csv", "record_mode": "grouped"}),
    );
    assert_eq!(
        codes(&value),
        [
            (
                ProblemCode::DuplicateId,
                "channels[2].source.settings.columns[1].name".to_owned()
            ),
            (
                ProblemCode::Schema,
                "channels[2].source.settings.group_by".to_owned()
            ),
            (
                ProblemCode::Schema,
                "channels[2].destinations[0].settings.group_by".to_owned()
            ),
        ]
    );
}

#[test]
fn unresolved_secrets_are_warnings() {
    let mut value = sample_value();
    value["endpoints"].as_array_mut().unwrap().push(json!({
        "endpoint_id": "partner", "label": "Partner SFTP", "kind": "sftp",
        "settings": {"host": "sftp.example.org", "username": "lab",
            "auth": {"method": "key", "private_key": {"secret": "partner-key"},
                "passphrase": {"secret": "partner-pass"}},
            "host_key_fingerprint": "SHA256:abcdefghijklmnop", "remote_directory": "/in"},
    }));
    let config = RuntimeConfig::from_value(value).unwrap();
    let problems = packie_config::unresolved_secrets(&config, &["partner-key".to_owned()]);
    let found: Vec<(ProblemCode, ProblemSeverity, &str)> = problems
        .iter()
        .map(|p| (p.code, p.severity, p.path.as_str()))
        .collect();
    assert_eq!(
        found,
        [(
            ProblemCode::SecretUnresolved,
            ProblemSeverity::Warning,
            "endpoints[4].settings.auth.passphrase"
        )]
    );
}

#[test]
fn au_class_a_destinations_default_to_handed_off() {
    let au = |class: &str| {
        json!({
            "profile": "au640", "frame_timeout_ms": 200, "block_timeout_ms": 1000,
            "framing": {"start": [2], "end": [3], "bcc": false, "use_etb": false,
                "max_wire_len": 512, "class": class},
            "format": {"unit_no": false, "rack_digits": 4, "sample_id_digits": 10, "sex": false,
                "age": false, "month": false, "patient_info_digits": [0, 0, 0, 0, 0, 0],
                "result_digits": 6},
        })
    };
    let with = |class: &str, required: Option<&str>| {
        let mut value = sample_value();
        let destination = &mut value["channels"][1]["destinations"][0];
        destination["protocol"] = json!("au");
        destination["settings"] = au(class);
        destination["mapping"] = json!({"rules": []});
        destination["delivery"] = match required {
            Some(level) => json!({"ack_required": level}),
            None => json!({}),
        };
        value
    };
    let effective = |value: serde_json::Value| {
        let config = RuntimeConfig::from_value(value).unwrap();
        let ChannelConfig::Message(channel) = &config.channels[1] else {
            panic!("channel 1 is a message channel");
        };
        channel.destinations[0]
            .effective_delivery(&channel.delivery)
            .ack_required
    };
    use packie_core::AckLevel;
    assert_eq!(effective(with("a", None)), AckLevel::HandedOff);
    assert_eq!(effective(with("b", None)), AckLevel::Received);

    let rejected = RuntimeConfig::from_value(with("a", Some("received"))).unwrap_err();
    let text = format!("{rejected}");
    assert!(
        text.contains("channels[1].destinations[0].delivery.ack_required")
            && text.contains("au class A sends no acknowledgement"),
        "{text}"
    );
}

#[test]
fn trailing_text_and_overly_precise_multipliers_are_rejected() {
    let text = std::fs::read_to_string(repo().join("config/runtime.json")).unwrap();
    assert!(RuntimeConfig::parse(&format!("{text}\n  \n")).is_ok());
    let errors = RuntimeConfig::parse(&format!("{text} {{}}")).unwrap_err();
    let problem = &errors.problems()[0];
    assert_eq!(problem.code, ProblemCode::Schema);
    assert!(
        problem
            .message
            .contains("unexpected text after the document"),
        "{problem:?}"
    );

    let mut value = sample_value();
    value["channels"][0]["retry"] = json!({"multiplier": 1.5});
    assert!(RuntimeConfig::from_value(value.clone()).is_ok());
    value["channels"][0]["retry"] = json!({"multiplier": 1.12345});
    assert_eq!(
        codes(&value),
        [(
            ProblemCode::RetryInvalid,
            "channels[0].retry.multiplier".to_owned()
        )]
    );
}

#[test]
fn a_busy_path_is_retried_on_windows_only() {
    let busy = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let mut calls = 0;
    let result = packie_config::retry_while_busy(|| {
        calls += 1;
        if calls < 3 { Err(busy()) } else { Ok(calls) }
    });
    if cfg!(windows) {
        assert_eq!(result.unwrap(), 3);
    } else {
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(calls, 1);
    }

    // Other errors are never retried.
    let mut calls = 0;
    let result: std::io::Result<()> = packie_config::retry_while_busy(|| {
        calls += 1;
        Err(std::io::ErrorKind::NotFound.into())
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
}
