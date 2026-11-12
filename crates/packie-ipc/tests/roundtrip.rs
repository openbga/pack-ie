//! Server and client over a real local socket: requests, errors, subscriptions.
#![cfg(test)]

use std::sync::Arc;

use packie_ipc::{
    ErrorCode, Event, Handler, IpcClient, IpcError, IpcServer, Request, Response, Topic,
};
use tokio::sync::mpsc;

struct Echo;

impl Handler for Echo {
    async fn handle(&self, request: Request) -> Result<Response, IpcError> {
        match request {
            Request::Hello { client } => Ok(Response::Hello {
                engine_version: "test".into(),
                protocol_version: packie_ipc::PROTOCOL_VERSION,
                node_id: client,
                node_name: "Test".into(),
                capabilities: Vec::new(),
                actor: None,
            }),
            _ => Err(IpcError::new(ErrorCode::NotImplemented, "not here")),
        }
    }

    fn subscribe(&self, topic: Topic) -> Result<mpsc::Receiver<Event>, IpcError> {
        if topic != Topic::Alerts {
            return Err(IpcError::new(ErrorCode::NotFound, "no such topic"));
        }
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            for missed in 1..=3 {
                let _ = tx.send(Event::Lagged { missed }).await;
            }
        });
        Ok(rx)
    }
}

fn endpoint(dir: &tempfile::TempDir) -> String {
    if cfg!(windows) {
        format!(r"\\.\pipe\packie-test-{}", std::process::id())
    } else {
        dir.path()
            .join("packie.sock")
            .to_string_lossy()
            .into_owned()
    }
}

#[tokio::test]
async fn requests_errors_and_subscriptions() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir);
    let server = IpcServer::bind(&endpoint).unwrap();
    let serving = tokio::spawn(server.serve(Arc::new(Echo)));

    let client = IpcClient::connect(&endpoint).await.unwrap();
    let hello = client
        .request(Request::Hello {
            client: "me".into(),
        })
        .await
        .unwrap();
    assert!(matches!(hello, Response::Hello { node_id, .. } if node_id == "me"));

    let err = client.request(Request::GetSnapshot).await.unwrap_err();
    assert!(
        matches!(err, packie_ipc::ClientError::Server(e) if e.code == ErrorCode::NotImplemented)
    );

    let mut alerts = client.subscribe(Topic::Alerts).await.unwrap();
    for expected in 1..=3 {
        assert_eq!(
            alerts.next().await,
            Some(Event::Lagged { missed: expected })
        );
    }
    let done = client
        .request(Request::Unsubscribe {
            subscription: alerts.id,
        })
        .await
        .unwrap();
    assert_eq!(done, Response::Done);
    let missing = client
        .request(Request::Unsubscribe { subscription: 99 })
        .await
        .unwrap_err();
    assert!(matches!(missing, packie_ipc::ClientError::Server(e) if e.code == ErrorCode::NotFound));

    // A second engine cannot take the endpoint while the first serves it.
    assert!(matches!(
        IpcServer::bind(&endpoint),
        Err(packie_ipc::TransportError::InUse(_))
    ));

    serving.abort();
    let _ = serving.await;
    // The socket is removed with the server, so the next start binds cleanly.
    IpcServer::bind(&endpoint).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_frames_get_bad_request_with_the_id() {
    use packie_ipc::ServerFrame;
    use packie_ipc::frame::{read_json, write_frame};
    use tokio::net::UnixStream;

    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir);
    let server = IpcServer::bind(&endpoint).unwrap();
    let serving = tokio::spawn(server.serve(Arc::new(Echo)));

    let mut stream = UnixStream::connect(&endpoint).await.unwrap();
    write_frame(
        &mut stream,
        br#"{"id": 7, "request": {"type": "launch_rockets"}}"#,
    )
    .await
    .unwrap();
    let frame: ServerFrame = read_json(&mut stream).await.unwrap().unwrap();
    let ServerFrame::Error { id, error } = frame else {
        panic!("expected an error, got {frame:?}");
    };
    assert_eq!((id, error.code), (7, ErrorCode::BadRequest));

    write_frame(&mut stream, b"not json").await.unwrap();
    let frame: ServerFrame = read_json(&mut stream).await.unwrap().unwrap();
    assert!(matches!(frame, ServerFrame::Error { id: 0, .. }));
    serving.abort();
}

#[cfg(unix)]
#[test]
fn socket_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _server = runtime.block_on(async { IpcServer::bind(&endpoint).unwrap() });
    let mode = std::fs::metadata(&endpoint).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
