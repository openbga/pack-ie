//! Static validation of a resolved scenario: what can be known before anything starts
//! (actor references and kinds, step and actor compatibility, required capabilities,
//! payload forms, async operation names, engine start/stop order).

use std::collections::BTreeSet;
use std::path::Path;

use crate::matcher::parse_path;
use crate::scenario::{
    Actor, Capability, ENGINE_ACTOR, ResolvedScenario, Scenario, StartMode, Step,
};
use crate::steps::{
    Action, ActorRule, Codes, ExpectMessageArgs, Protocol, SendMessageArgs, Target,
};

/// Checks `resolved` (built from `scenario`); `base` is the scenario file's directory.
pub(crate) fn check(
    scenario: &Scenario,
    resolved: &ResolvedScenario,
    base: &Path,
) -> Result<(), String> {
    check_capabilities(scenario, resolved)?;
    check_actors(scenario, resolved)?;
    let mut ops = BTreeSet::new();
    let mut engine_running = resolved.engine.start == StartMode::Auto;
    let mut engine_ever_started = engine_running;
    for step in &resolved.steps {
        let at = |m: String| format!("steps[{}] ({}): {m}", step.index, step.action.name());
        check_actor_rule(resolved, step).map_err(&at)?;
        if step.action.needs_test_hooks() && !resolved.requires.contains(&Capability::TestHooks) {
            return Err(at(
                "needs `test_hooks`, which the scenario does not list in `requires`".into(),
            ));
        }
        check_action(resolved, step, base, &mut ops).map_err(&at)?;
        match &step.action {
            Action::StartEngine(_) if engine_running => {
                return Err(at("the engine is already running".into()));
            }
            Action::StartEngine(_) => {
                engine_running = true;
                engine_ever_started = true;
            }
            Action::StopEngine(_) | Action::CrashEngine(_) if !engine_running => {
                return Err(at("the engine is not running".into()));
            }
            Action::StopEngine(_) | Action::CrashEngine(_) => engine_running = false,
            other if needs_running_engine(other) && !engine_running => {
                return Err(at(if engine_ever_started {
                    "needs a running engine, but it was stopped".into()
                } else {
                    "needs a running engine; with `start: manual` add a `start_engine` step first"
                        .into()
                }));
            }
            _ => {}
        }
    }
    for sentinel in &resolved.no_body_leak {
        if sentinel.is_empty() {
            return Err("no_body_leak: sentinels must not be empty".into());
        }
    }
    Ok(())
}

/// Steps answered by the running engine over its management interface.
fn needs_running_engine(action: &Action) -> bool {
    matches!(
        action,
        Action::AssertSnapshot(_)
            | Action::AssertAlert(_)
            | Action::PauseChannel(_)
            | Action::ResumeChannel(_)
            | Action::RestartChannel(_)
            | Action::Resend(_)
            | Action::CloseDelivery(_)
            | Action::ValidateConfig(_)
            | Action::ApplyConfig(_)
            | Action::HoldStoreCommits(_)
            | Action::ReleaseStoreCommits(_)
    )
}

fn check_capabilities(scenario: &Scenario, resolved: &ResolvedScenario) -> Result<(), String> {
    let needs = |c: Capability| resolved.requires.contains(&c);
    let uses_serial = !scenario.resources.serial_pairs.is_empty()
        || resolved
            .actors
            .values()
            .any(|a| matches!(a, Actor::SerialPeer(_)));
    if uses_serial && !needs(Capability::SerialPair) {
        return Err("serial pairs are used but `requires` does not list `serial_pair`".into());
    }
    let alias = |addr: &str| addr.starts_with("127.") && addr != "127.0.0.1";
    for (name, actor) in &resolved.actors {
        let address = match actor {
            Actor::TcpServer(s) => Some(s.bind.as_str()),
            Actor::TcpClient(c) => c.bind_source.as_deref(),
            _ => None,
        };
        if address.is_some_and(alias) && !needs(Capability::LoopbackAlias) {
            return Err(format!(
                "actors.{name}: address {} needs `loopback_alias` in `requires`",
                address.unwrap_or_default()
            ));
        }
    }
    Ok(())
}

fn check_actors(scenario: &Scenario, resolved: &ResolvedScenario) -> Result<(), String> {
    for (name, actor) in &resolved.actors {
        match actor {
            Actor::SerialPeer(s) => {
                if !scenario.resources.serial_pairs.contains(&s.pair) {
                    return Err(format!(
                        "actors.{name}: pair `{}` is not declared in resources.serial_pairs",
                        s.pair
                    ));
                }
                if !matches!(s.parity.as_str(), "none" | "even" | "odd") {
                    return Err(format!(
                        "actors.{name}: parity `{}` is not none, even or odd",
                        s.parity
                    ));
                }
                if !matches!(s.stop_bits.as_str(), "one" | "two") {
                    return Err(format!(
                        "actors.{name}: stop_bits `{}` is not one or two",
                        s.stop_bits
                    ));
                }
                if !matches!(s.flow_control.as_str(), "none" | "rts_cts" | "xon_xoff") {
                    return Err(format!(
                        "actors.{name}: flow_control `{}` is not none, rts_cts or xon_xoff",
                        s.flow_control
                    ));
                }
                if !(5..=8).contains(&s.data_bits) {
                    return Err(format!("actors.{name}: data_bits must be 5 to 8"));
                }
            }
            Actor::Folder(f) => {
                if !scenario.resources.folders.contains(&f.resource) {
                    return Err(format!(
                        "actors.{name}: resource `{}` is not declared in resources.folders",
                        f.resource
                    ));
                }
            }
            Actor::TcpServer(_) | Actor::TcpClient(_) => {}
        }
    }
    Ok(())
}

fn check_actor_rule(resolved: &ResolvedScenario, step: &Step) -> Result<(), String> {
    let rule = step.action.actor_rule();
    if step.actor == ENGINE_ACTOR {
        return match rule {
            ActorRule::Engine | ActorRule::Any => Ok(()),
            _ => Err("is an actor step; `actor` must name an actor, not `engine`".into()),
        };
    }
    let actor = resolved
        .actors
        .get(&step.actor)
        .ok_or_else(|| format!("unknown actor `{}`", step.actor))?;
    let ok = match rule {
        ActorRule::Engine => false,
        ActorRule::TcpServer => matches!(actor, Actor::TcpServer(_)),
        ActorRule::Connector => matches!(actor, Actor::TcpClient(_) | Actor::SerialPeer(_)),
        ActorRule::Connection => actor.has_connection(),
        ActorRule::Folder => matches!(actor, Actor::Folder(_)),
        ActorRule::Any => true,
    };
    if ok {
        Ok(())
    } else {
        Err(match rule {
            ActorRule::Engine => "is an engine step; `actor` must be `engine`".to_owned(),
            _ => format!(
                "cannot run on actor `{}` of kind {}",
                step.actor,
                actor.kind_name()
            ),
        })
    }
}

fn check_action(
    resolved: &ResolvedScenario,
    step: &Step,
    base: &Path,
    ops: &mut BTreeSet<String>,
) -> Result<(), String> {
    match &step.action {
        Action::Connect(a) => match resolved.actors.get(&step.actor) {
            Some(Actor::TcpClient(_)) if a.host.is_none() || a.port.is_none() => {
                Err("a tcp_client connect needs `host` and `port`".into())
            }
            Some(Actor::SerialPeer(_))
                if a.host.is_some()
                    || a.port.is_some()
                    || a.expect != crate::steps::ConnectExpect::Ok =>
            {
                Err("a serial_peer connect takes no fields".into())
            }
            _ => Ok(()),
        },
        Action::Send(_)
        | Action::Expect(_)
        | Action::SendMessage(_)
        | Action::ExpectMessage(_)
        | Action::AwaitSend(_) => check_traffic_action(&step.action, ops),
        Action::WriteFile(_) | Action::ExpectFile(_) | Action::DeleteFile(_) => {
            check_folder_action(&step.action)
        }
        _ => check_engine_action(&step.action, base),
    }
}

fn payload_ok(spec: &crate::payload::PayloadSpec) -> Result<(), String> {
    spec.check().map_err(|e| e.to_string())
}

/// Steps that send, expect or await bytes and messages.
fn check_traffic_action(action: &Action, ops: &mut BTreeSet<String>) -> Result<(), String> {
    match action {
        Action::Send(a) => {
            payload_ok(&a.payload())?;
            if a.chunk_size == Some(0) {
                return Err("chunk_size must be greater than 0".into());
            }
            register_op(ops, a.is_async, a.name.as_deref())
        }
        Action::Expect(a) => payload_ok(&a.payload()),
        Action::SendMessage(a) => {
            check_send_message(a)?;
            register_op(ops, a.is_async, a.name.as_deref())
        }
        Action::ExpectMessage(a) => check_expect_message(a),
        Action::AwaitSend(a) if ops.contains(&a.name) => Ok(()),
        Action::AwaitSend(a) => Err(format!("no earlier async step is named `{}`", a.name)),
        _ => Ok(()),
    }
}

/// Steps on a folder actor.
fn check_folder_action(action: &Action) -> Result<(), String> {
    match action {
        Action::WriteFile(a) => {
            payload_ok(&a.payload())?;
            check_file_name(&a.name)
        }
        Action::ExpectFile(a) => {
            match (&a.name, &a.pattern) {
                (Some(_), None) | (None, Some(_)) => {}
                _ => return Err("exactly one of `name` and `pattern` is required".into()),
            }
            if let Some(name) = &a.name {
                check_file_name(name)?;
            }
            if let Some(equals) = &a.equals {
                payload_ok(equals)?;
                if a.contains.is_some() {
                    return Err("`equals` and `contains` are exclusive".into());
                }
            }
            Ok(())
        }
        Action::DeleteFile(a) => check_file_name(&a.name),
        _ => Ok(()),
    }
}

/// Configuration, selector and assertion steps.
fn check_engine_action(action: &Action, base: &Path) -> Result<(), String> {
    match action {
        Action::ValidateConfig(a) => {
            check_runtime_source(a.runtime.as_ref(), a.runtime_file.as_deref(), base)
        }
        Action::ApplyConfig(a) => {
            check_runtime_source(a.runtime.as_ref(), a.runtime_file.as_deref(), base)
        }
        Action::AssertSnapshot(a) => {
            if matches!(a.target, Target::Node { node: false }) {
                return Err("target `node` must be true".into());
            }
            if a.expect.is_empty() {
                return Err("`expect` must not be empty".into());
            }
            check_paths(a.expect.keys())
        }
        Action::AssertAlert(a) => check_paths(a.expect.keys()),
        Action::AssertStore(a) if a.where_.is_empty() && a.each.is_empty() && a.count.is_none() => {
            Err("needs `where`, `each` or `count`".into())
        }
        Action::CloseDelivery(a) if a.delivery.is_empty() => {
            Err("`delivery` selector must not be empty".into())
        }
        Action::Resend(a) if a.message.is_empty() => {
            Err("`message` selector must not be empty".into())
        }
        _ => Ok(()),
    }
}

fn register_op(
    ops: &mut BTreeSet<String>,
    is_async: bool,
    name: Option<&str>,
) -> Result<(), String> {
    match (is_async, name) {
        (true, Some(name)) => {
            if ops.insert(name.to_owned()) {
                Ok(())
            } else {
                Err(format!("async name `{name}` is used twice"))
            }
        }
        (true, None) => Err("`async` needs a `name`".into()),
        (false, Some(_)) => Err("`name` is only used with `async`".into()),
        (false, None) => Ok(()),
    }
}

fn check_file_name(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    let escapes = path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    if name.is_empty() || escapes {
        Err(format!(
            "file name `{name}` must be a relative path inside the folder"
        ))
    } else {
        Ok(())
    }
}

fn check_paths<'a>(paths: impl Iterator<Item = &'a String>) -> Result<(), String> {
    for path in paths {
        parse_path(path)?;
    }
    Ok(())
}

fn check_runtime_source(
    inline: Option<&serde_json::Value>,
    file: Option<&str>,
    base: &Path,
) -> Result<(), String> {
    match (inline, file) {
        (Some(_), None) => Ok(()),
        (None, Some(file)) => {
            if base.join(file).is_file() {
                Ok(())
            } else {
                Err(format!(
                    "runtime_file `{file}` does not exist next to the scenario"
                ))
            }
        }
        _ => Err("exactly one of `runtime` and `runtime_file` is required".into()),
    }
}

const HL7_CODES: [&str; 6] = ["AA", "AE", "AR", "CA", "CE", "CR"];

fn check_codes(codes: &Codes) -> Result<(), String> {
    if let Codes::List(list) = codes {
        if list.is_empty() {
            return Err("an empty code list".into());
        }
        for code in list {
            if !HL7_CODES.contains(&code.as_str()) {
                return Err(format!(
                    "unknown acknowledgement code `{code}` (expected one of {})",
                    HL7_CODES.join(", ")
                ));
            }
        }
    }
    Ok(())
}

fn check_send_message(a: &SendMessageArgs) -> Result<(), String> {
    match a.protocol {
        Protocol::Hl7Mllp => {
            if a.segments.as_ref().is_none_or(Vec::is_empty) {
                return Err("hl7_mllp needs a non-empty `segments`".into());
            }
            if a.records.is_some() || a.partial.is_some() {
                return Err("`records` and `partial` belong to astm".into());
            }
            if let Some(codes) = &a.expect_reply {
                check_codes(codes)?;
            }
        }
        Protocol::Astm => {
            if a.records.as_ref().is_none_or(Vec::is_empty) {
                return Err("astm needs a non-empty `records`".into());
            }
            if a.segments.is_some() || a.expect_reply.is_some() {
                return Err("`segments` and `expect_reply` belong to hl7_mllp".into());
            }
        }
    }
    Ok(())
}

fn check_expect_message(a: &ExpectMessageArgs) -> Result<(), String> {
    for (i, entry) in a.assert.iter().enumerate() {
        let given = usize::from(a.assert[i].equals.is_some())
            + usize::from(entry.matches.is_some())
            + usize::from(entry.exists.is_some());
        if given != 1 {
            return Err(format!(
                "assert[{i}] needs exactly one of equals, matches, exists"
            ));
        }
        if let Some(re) = &entry.matches {
            regex::Regex::new(re).map_err(|e| format!("assert[{i}].matches: {e}"))?;
        }
    }
    match a.protocol {
        Protocol::Hl7Mllp => {
            if a.nak_frames.is_some() {
                return Err("`nak_frames` belongs to astm".into());
            }
            if let Some(codes) = &a.reply {
                check_codes(codes)?;
            }
        }
        Protocol::Astm => {
            if a.reply_delay_ms.is_some() {
                return Err("`reply_delay_ms` belongs to hl7_mllp".into());
            }
            match &a.reply {
                None => {}
                Some(Codes::None) => {
                    if !a.assert.is_empty() {
                        return Err(
                            "with `reply: none` no transfer completes, so `assert` cannot apply"
                                .into(),
                        );
                    }
                }
                Some(Codes::List(l)) if l.len() == 1 && l[0] == "ack" => {}
                Some(_) => return Err("astm `reply` is `ack` or `none`".into()),
            }
        }
    }
    Ok(())
}
