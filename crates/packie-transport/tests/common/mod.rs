//! Shared helpers for the transport tests.
#![allow(dead_code, unreachable_pub, clippy::panic, clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use packie_config::EndpointConfig;
use packie_core::{ChannelId, EndpointId};
use packie_transport::{Bind, Dialer, MapSecrets, Opener, SystemTransports, Transports};
use serde_json::Value;

/// An endpoint from its settings JSON with the `kind` inside, for brevity.
pub fn endpoint(mut transport: Value) -> EndpointConfig {
    let kind = transport
        .as_object_mut()
        .and_then(|t| t.remove("kind"))
        .unwrap();
    serde_json::from_value(serde_json::json!({
        "endpoint_id": EndpointId::generate(),
        "label": "Test endpoint",
        "kind": kind,
        "settings": transport,
    }))
    .unwrap()
}

/// `base` with the fields of the object `extra` added or replaced.
pub fn merged(mut base: Value, extra: &Value) -> Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    base
}

pub fn transports() -> SystemTransports {
    SystemTransports::new(Arc::new(MapSecrets::default()))
}

pub fn holder() -> ChannelId {
    ChannelId::generate()
}

/// The dialer of a TCP client or serial endpoint.
pub fn dialer(
    transports: &SystemTransports,
    config: &EndpointConfig,
    holder: ChannelId,
) -> Box<dyn Dialer> {
    match transports.opener(config, holder).unwrap() {
        Opener::Dial(dialer) => dialer,
        Opener::Listen(_) => panic!("expected a dialer"),
    }
}

/// The binder of a TCP listener endpoint.
pub fn binder(config: &EndpointConfig) -> Box<dyn Bind> {
    match transports().opener(config, holder()).unwrap() {
        Opener::Listen(bind) => bind,
        Opener::Dial(_) => panic!("expected a listener"),
    }
}

/// The sorted names of the entries of `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
