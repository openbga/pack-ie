//! Setup checks over IPC: the serial ports of the computer, read-only endpoint probes and
//! test notifications (`docs/spec/diagnostics.md` §6.1, §10); and the alert notifier.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use packie_config::{
    Bootstrap, ChannelConfig, EndpointConfig, RuntimeConfig, SecretResolver, Transport,
};
use packie_core::{ChannelId, EndpointId};
use packie_diagnostics::{Alert, FailingTarget};
use packie_ipc::checks::{
    DEFAULT_PROBE_SECONDS, EndpointTest, MAX_PROBE_SECONDS, NotificationResult, PortUse,
};
use packie_ipc::{ErrorCode, IpcError, Response};
use packie_notify::{Labels, Notifier};
use serde_json::json;

use crate::engine::Shared;
use crate::handler::registry_error;

/// The id a probed definition gets; it is never stored.
const PROBE_ID: &str = "endpoint-test";

/// `ListSerialPorts`: what the OS reports, with the configured endpoints naming each port.
pub(crate) async fn serial_ports(shared: &Shared) -> Result<Response, IpcError> {
    let mut ports = shared
        .registry
        .serial_ports()
        .await
        .map_err(registry_error)?;
    let config = shared.active();
    let held: HashMap<EndpointId, ChannelId> = shared
        .registry
        .snapshot()
        .endpoints
        .iter()
        .filter_map(|e| e.held_by.map(|channel| (e.id, channel)))
        .collect();
    for port in &mut ports {
        port.used_by = port_uses(&config, &held, &port.name);
    }
    Ok(Response::SerialPorts { ports })
}

/// The serial endpoints of `config` that name `port`.
fn port_uses(
    config: &RuntimeConfig,
    held: &HashMap<EndpointId, ChannelId>,
    port: &str,
) -> Vec<PortUse> {
    config
        .endpoints
        .iter()
        .filter(|e| match &e.transport {
            Transport::Serial(s) => s.virtual_link.is_none() && same_port(&s.port_name, port),
            _ => false,
        })
        .map(|e| PortUse {
            endpoint_id: e.id,
            endpoint_label: e.label.clone(),
            channels: config
                .channels
                .iter()
                .filter(|c| c.endpoint_ids().contains(&e.id))
                .map(ChannelConfig::id)
                .collect(),
            held_by: held.get(&e.id).copied(),
        })
        .collect()
}

/// Port names compare without case on Windows (`com3` is `COM3`), exactly elsewhere.
fn same_port(configured: &str, reported: &str) -> bool {
    if cfg!(windows) {
        configured.eq_ignore_ascii_case(reported)
    } else {
        configured == reported
    }
}

/// `TestEndpoint`: validates the definition like a document's endpoint, then probes it.
pub(crate) async fn test_endpoint(
    shared: &Shared,
    test: EndpointTest,
) -> Result<Response, IpcError> {
    let seconds = test.timeout_seconds.unwrap_or(DEFAULT_PROBE_SECONDS);
    if !(1..=MAX_PROBE_SECONDS).contains(&seconds) {
        return Err(IpcError::new(
            ErrorCode::BadRequest,
            format!("timeout_seconds must be from 1 to {MAX_PROBE_SECONDS}."),
        ));
    }
    let endpoint = definition(test)?;
    let report = shared
        .registry
        .probe_endpoint(&endpoint, Duration::from_secs(u64::from(seconds)))
        .await
        .map_err(registry_error)?;
    Ok(Response::EndpointTested(report))
}

/// The endpoint `test` describes, checked by the document's rules; problem paths are
/// relative to the endpoint (`settings.port`).
fn definition(test: EndpointTest) -> Result<EndpointConfig, IpcError> {
    let document = json!({ "endpoints": [{
        "endpoint_id": PROBE_ID, "label": "Endpoint test",
        "kind": test.kind, "settings": test.settings,
    }]});
    let config = RuntimeConfig::from_value(document).map_err(|errors| {
        let mut problems = errors.into_problems();
        for problem in &mut problems {
            if let Some(path) = problem.path.strip_prefix("endpoints[0].") {
                problem.path = path.to_owned();
            }
        }
        let count = problems.iter().filter(|p| p.is_error()).count();
        IpcError {
            message: format!(
                "The endpoint definition has {count} error{}; nothing was tested.",
                if count == 1 { "" } else { "s" }
            ),
            ..IpcError::invalid_config(problems)
        }
    })?;
    config
        .endpoints
        .into_iter()
        .next()
        .ok_or_else(|| IpcError::new(ErrorCode::Internal, "The endpoint definition was lost."))
}

/// The audited target of a probe: where it points, from the non-secret settings.
pub(crate) fn probe_target(test: &EndpointTest) -> String {
    let field = |name: &str| test.settings.get(name).map(ToString::to_string);
    let place = ["host", "bind_address", "port_name", "path"]
        .iter()
        .find_map(|name| field(name))
        .unwrap_or_default();
    let place = place.trim_matches('"');
    match field("port") {
        Some(port) => format!("{place}:{port}"),
        None => place.to_owned(),
    }
}

/// `TestNotification`: one test message per configured target.
pub(crate) async fn test_notification(shared: &Shared) -> Response {
    let results = shared
        .notifier
        .test()
        .await
        .into_iter()
        .map(|r| NotificationResult {
            target: r.target.to_owned(),
            ok: r.ok,
            detail: r.detail,
        })
        .collect();
    shared.snapshot_now.notify_one();
    Response::NotificationTested { results }
}

/// The alert notifier of the bootstrap's `notifications` section. Logs a warning for each
/// notification secret the store does not hold.
pub(crate) fn notifier(bootstrap: &Bootstrap, secrets: &Arc<dyn SecretResolver>) -> Notifier {
    let settings = &bootstrap.notifications;
    let refs = settings.secret_refs();
    if !refs.is_empty() {
        let names = secrets.names().unwrap_or_default();
        for (path, secret) in refs {
            if !names.iter().any(|n| n == secret.name()) {
                tracing::warn!(
                    event = "notification.secret_unresolved",
                    field = path,
                    secret = secret.name(),
                    "Notification secret {} ({path}) is not in the secret store; set it with `packie secret set`",
                    secret.name()
                );
            }
        }
    }
    Notifier::new(
        settings.clone(),
        Arc::clone(secrets),
        bootstrap.node.display_name.clone(),
    )
}

/// Hands an alert change to the notifier with its channel and endpoint labels.
pub(crate) fn notify(notifier: &Notifier, config: &RuntimeConfig, alert: &Alert) {
    let labels = Labels {
        channel: alert
            .channel_id
            .and_then(|id| config.channel(id))
            .map(|c| c.label().to_owned()),
        endpoint: alert
            .endpoint_id
            .and_then(|id| config.endpoint(id))
            .map(|e| e.label.clone()),
    };
    notifier.alert_changed(alert, &labels);
}

/// The notifier's failing targets, for the `notifications_failing` alert.
pub(crate) fn failing(notifier: &Notifier) -> Vec<FailingTarget> {
    notifier
        .failing()
        .into_iter()
        .map(|f| FailingTarget {
            target: f.target.to_owned(),
            since: f.since,
            error: f.error,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use packie_core::EndpointKind;

    use super::*;

    fn test(kind: EndpointKind, settings: serde_json::Value) -> EndpointTest {
        EndpointTest {
            kind,
            settings,
            timeout_seconds: None,
        }
    }

    #[test]
    fn a_definition_is_checked_like_a_document_endpoint() {
        let ok = definition(test(
            EndpointKind::TcpClient,
            json!({ "host": "lis.example.test", "port": 5000 }),
        ))
        .unwrap();
        assert_eq!(ok.kind(), EndpointKind::TcpClient);
        let error = definition(test(EndpointKind::TcpClient, json!({ "port": 5000 }))).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidConfig);
        assert!(
            error
                .field_errors
                .iter()
                .all(|p| !p.path.starts_with("endpoints")),
            "{:?}",
            error.field_errors
        );
    }

    #[test]
    fn the_audited_target_names_the_place_without_secrets() {
        let sftp = test(
            EndpointKind::Sftp,
            json!({ "host": "sftp.example.test", "port": 22, "username": "lab",
                    "auth": { "password": { "secret": "sftp.pw" } } }),
        );
        assert_eq!(probe_target(&sftp), "sftp.example.test:22");
        let serial = test(
            EndpointKind::Serial,
            json!({ "port_name": "COM3", "baud_rate": 9600 }),
        );
        assert_eq!(probe_target(&serial), "COM3");
    }
}
