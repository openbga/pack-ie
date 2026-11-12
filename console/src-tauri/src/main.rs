//! Desktop shell for the PACK-IE Console: one Tauri window over the bundled UI.
//!
//! The shell holds one connection to the engine (`docs/architecture.md` §1) and passes
//! requests, subscriptions and connection changes through to the window; it keeps no engine
//! logic of its own. The window reaches Rust only through the commands in the handler list
//! below, granted by `core:default` in `capabilities/main.json`. The dialog plugin is used
//! from Rust only (`export.rs`); the window is granted no plugin permission.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod connection;
mod endpoint;
mod error;
mod export;
mod service;
mod state;
mod supervisor;

#[cfg(test)]
mod connection_tests;

use std::sync::Arc;

use tauri::{AppHandle, Emitter as _, Manager};
use tracing_subscriber::EnvFilter;

use crate::connection::Hub;
use crate::state::{Backoff, ConnectionState, Emitter, EventPayload};

/// Event carrying every engine subscription event.
const EVENT_NAME: &str = "packie://event";
/// Event carrying every connection change.
const CONNECTION_EVENT: &str = "packie://connection";

/// Emits to every window.
struct TauriEmitter(AppHandle);

impl Emitter for TauriEmitter {
    fn connection(&self, state: &ConnectionState) {
        if let Err(error) = self.0.emit(CONNECTION_EVENT, state) {
            tracing::warn!(%error, "cannot emit the connection state");
        }
    }

    fn event(&self, payload: &EventPayload<'_>) {
        if let Err(error) = self.0.emit(EVENT_NAME, payload) {
            tracing::warn!(%error, "cannot emit an engine event");
        }
    }
}

fn main() {
    // Logs go to stderr, filtered by `RUST_LOG` (default: info). Never message bodies.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_file = app
                .path()
                .app_config_dir()
                .ok()
                .map(|dir| dir.join("console.json"));
            let emitter = Arc::new(TauriEmitter(app.handle().clone()));
            let hub = Hub::new(emitter, config_file, Backoff::default());
            app.manage(hub.clone());
            tauri::async_runtime::spawn(hub.run());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::ipc_request,
            commands::ipc_subscribe,
            commands::ipc_unsubscribe,
            commands::ipc_connection,
            commands::ipc_configure,
            commands::read_bootstrap,
            commands::bootstrap_file,
            commands::start_service,
            export::save_audit_export,
            export::save_message_export,
            export::save_report_file,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        tracing::error!(%error, "the console failed to start");
        std::process::exit(1);
    }
}
