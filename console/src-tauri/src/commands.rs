//! The commands the window may call. Each is a thin adapter over [`Hub`]; names and payloads
//! are the contract with the front end (`console/src/ipc`).

use tauri::{AppHandle, State};
use packie_ipc::{Request, Response, Topic};

use crate::connection::Hub;
use crate::error::CommandError;
use crate::service;
use crate::state::ConnectionState;

/// The console's own version: the `version` of `tauri.conf.json`, which is also the installer's
/// (a test keeps it equal to this crate's manifest and the package's).
#[tauri::command]
pub(crate) fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[cfg(test)]
mod tests {
    /// The version is written in three places that must agree: the Tauri configuration (what the
    /// About page shows and the installer carries), this crate's manifest (what the engine's
    /// log names in the greeting) and the web package.
    #[test]
    fn the_console_has_one_version_everywhere() {
        let json_version = |text: &str| -> String {
            let document: serde_json::Value = serde_json::from_str(text).unwrap();
            document["version"].as_str().unwrap().to_owned()
        };
        let tauri = json_version(include_str!("../tauri.conf.json"));
        assert_eq!(tauri, env!("CARGO_PKG_VERSION"));
        assert_eq!(tauri, json_version(include_str!("../../package.json")));
    }
}

/// Sends one request to the engine and returns its answer.
#[tauri::command]
pub(crate) async fn ipc_request(
    hub: State<'_, Hub>,
    request: Request,
) -> Result<Response, CommandError> {
    hub.request(request).await
}

/// Opens an engine subscription and returns its console id. Events arrive as
/// `packie://event`; the id and the subscription survive reconnects.
#[tauri::command]
pub(crate) async fn ipc_subscribe(hub: State<'_, Hub>, topic: Topic) -> Result<u64, CommandError> {
    hub.subscribe(topic).await
}

/// Stops a subscription.
#[tauri::command]
pub(crate) async fn ipc_unsubscribe(
    hub: State<'_, Hub>,
    subscription: u64,
) -> Result<(), CommandError> {
    hub.unsubscribe(subscription).await;
    Ok(())
}

/// The current connection state (also pushed as `packie://connection`).
#[tauri::command]
pub(crate) fn ipc_connection(hub: State<'_, Hub>) -> ConnectionState {
    hub.connection()
}

/// Sets (`Some`) or clears (`None`) the engine endpoint override and reconnects now.
#[tauri::command]
pub(crate) fn ipc_configure(
    hub: State<'_, Hub>,
    endpoint: Option<String>,
) -> Result<(), CommandError> {
    hub.configure(endpoint);
    Ok(())
}

/// The bootstrap document the endpoint is resolved from, when it is.
#[tauri::command]
pub(crate) async fn read_bootstrap(
    hub: State<'_, Hub>,
) -> Result<Option<serde_json::Value>, CommandError> {
    hub.bootstrap()
}

/// The path of the bootstrap file the endpoint is resolved from, when it is: what the
/// operator's `packie --bootstrap` commands must name to reach this engine.
#[tauri::command]
pub(crate) fn bootstrap_file(hub: State<'_, Hub>) -> Option<String> {
    hub.bootstrap_path()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Runs `packie service start` for the installation the console found; returns what it printed.
#[tauri::command]
pub(crate) async fn start_service(hub: State<'_, Hub>) -> Result<String, CommandError> {
    service::start_service(hub.bootstrap_path()).await
}
