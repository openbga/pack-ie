//! Socket permissions: owner-only, or shared with the configured group.
#![cfg(all(test, unix))]

use std::os::unix::fs::{MetadataExt, PermissionsExt};

use nix::unistd::{Group, getegid};
use packie_ipc::IpcServer;

#[tokio::test]
async fn socket_is_shared_with_an_existing_group_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("packie.sock");
    let endpoint = path.to_string_lossy().into_owned();
    let mode_and_gid = |path: &std::path::Path| {
        let metadata = std::fs::metadata(path).unwrap();
        (metadata.permissions().mode() & 0o777, metadata.gid())
    };

    let owner_only = IpcServer::bind(&endpoint).unwrap();
    assert_eq!(mode_and_gid(&path).0, 0o600);
    drop(owner_only);

    // Our own group always exists and we may give our files to it.
    let own = Group::from_gid(getegid()).unwrap().unwrap();
    let shared = IpcServer::bind_shared(&endpoint, Some(&own.name)).unwrap();
    assert_eq!(mode_and_gid(&path), (0o660, own.gid.as_raw()));
    drop(shared);

    let _missing = IpcServer::bind_shared(&endpoint, Some("packie-test-no-such-group")).unwrap();
    assert_eq!(mode_and_gid(&path).0, 0o600);
}
