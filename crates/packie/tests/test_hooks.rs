//! `packie run` with `test_hooks`: the capability is advertised and `crash` ends the process.
#![cfg(test)]
// The crash hook test drives a Unix socket; nothing in this file runs on Windows.
#![cfg(unix)]

mod common;

use packie_ipc::{Capability, IpcClient, Request, Response, TestHook};

#[test]
fn crash_hook_ends_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("state/packie.sock");
    let bootstrap = common::write_bootstrap(
        dir.path(),
        &serde_json::json!({
            "node": {"node_id": "hooks", "display_name": "Hooks"},
            "paths": {"state_directory": "state", "log_directory": "logs"},
            "test_hooks": true,
        }),
    );
    let mut engine = common::spawn_engine(&bootstrap);
    common::wait_for_engine(&bootstrap);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let client = IpcClient::connect(socket.to_str().unwrap()).await.unwrap();
        let hello = client
            .request(Request::Hello {
                client: "test".into(),
            })
            .await
            .unwrap();
        let Response::Hello { capabilities, .. } = hello else {
            panic!("expected hello");
        };
        assert_eq!(capabilities, [Capability::TestHooks]);
        for hook in [
            TestHook::HoldStoreCommits,
            TestHook::ReleaseStoreCommits,
            TestHook::Crash,
        ] {
            let answer = client.request(Request::TestHook { hook }).await.unwrap();
            assert_eq!(answer, Response::Done);
        }
    });
    let status = engine.wait().unwrap();
    assert_eq!(status.code(), Some(86), "crash exit code");
}
