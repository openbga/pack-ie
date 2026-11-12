//! An in-process SSH/SFTP server on localhost whose files live in a temporary directory.
//! Synthetic keys, user and password only.
#![allow(unreachable_pub, clippy::unwrap_used)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use packie_config::EndpointConfig;
use packie_transport::{FileSink, FileSource, MapSecrets, SystemTransports, Transports};
use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{HashAlg, PrivateKey, PublicKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File as SftpFile, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
    Version,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;

use crate::common::{endpoint, merged, names};

pub const USER: &str = "lab_user";
pub const PASSWORD: &str = "synthetic-sftp-password";

pub fn key(seed: u8) -> PrivateKey {
    PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
}

pub fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

/// A running server and the files behind it.
pub struct Server {
    pub fingerprint: String,
    port: u16,
    files: TempDir,
}

impl Server {
    /// Starts a server with `/inbound` and `/outbound`, accepting the synthetic password
    /// and the public key of `client`.
    pub async fn start(client: &PrivateKey) -> Self {
        let files = TempDir::new().unwrap();
        fs::create_dir(files.path().join("inbound")).unwrap();
        fs::create_dir(files.path().join("outbound")).unwrap();
        let host_key = key(7);
        let fingerprint = fingerprint(&host_key);
        let config = Arc::new(russh::server::Config {
            keys: vec![host_key],
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let root = files.path().to_owned();
        let allowed = client.public_key().clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let ssh = Ssh {
                    root: root.clone(),
                    allowed: allowed.clone(),
                    channels: HashMap::new(),
                };
                tokio::spawn(serve(Arc::clone(&config), socket, ssh));
            }
        });
        Self {
            fingerprint,
            port,
            files,
        }
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        self.files.path().join(name)
    }

    pub fn names(&self, name: &str) -> Vec<String> {
        names(&self.dir(name))
    }

    /// The endpoint for `remote_directory`, with `extra` transport fields on top of fast
    /// timings and password authentication.
    pub fn endpoint(&self, remote_directory: &str, extra: &Value) -> EndpointConfig {
        let base = json!({
            "kind": "sftp",
            "host": "127.0.0.1",
            "port": self.port,
            "username": USER,
            "auth": {"method": "password", "password": {"secret": "sftp-pw"}},
            "host_key_fingerprint": self.fingerprint,
            "remote_directory": remote_directory,
            "connect_timeout_ms": 5000,
            "operation_timeout_ms": 10000,
            "poll_interval_seconds": 1,
            "stable_seconds": 0,
        });
        endpoint(merged(base, extra))
    }

    pub fn sink(&self, extra: &Value) -> Box<dyn FileSink> {
        transports(PASSWORD)
            .file_sink(&self.endpoint("/outbound", extra))
            .unwrap()
    }

    pub fn source(&self, extra: &Value) -> Box<dyn FileSource> {
        transports(PASSWORD)
            .file_source(&self.endpoint("/inbound", extra))
            .unwrap()
    }
}

pub fn transports(password: &str) -> SystemTransports {
    let secrets = MapSecrets::new(HashMap::from([("sftp-pw".to_owned(), password.to_owned())]));
    SystemTransports::new(Arc::new(secrets))
}

/// One client's SSH session, until it ends.
async fn serve(config: Arc<russh::server::Config>, socket: tokio::net::TcpStream, ssh: Ssh) {
    if let Ok(session) = russh::server::run_stream(config, socket, ssh).await {
        let _ = session.await;
    }
}

struct Ssh {
    root: PathBuf,
    allowed: PublicKey,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl russh::server::Handler for Ssh {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(
            if user == USER && key.public_key.key_data() == self.allowed.key_data() {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match self.channels.remove(&id) {
            Some(channel) if name == "sftp" => {
                session.channel_success(id)?;
                let files = Files {
                    root: self.root.clone(),
                    open: HashMap::new(),
                    dirs: HashMap::new(),
                    next: 0,
                };
                russh_sftp::server::run(channel.into_stream(), files).await;
            }
            _ => session.channel_failure(id)?,
        }
        Ok(())
    }
}

/// SFTP onto the temporary directory. A rename never replaces a file, as OpenSSH's.
struct Files {
    root: PathBuf,
    open: HashMap<String, File>,
    /// Directory handles: the entries until they were read once.
    dirs: HashMap<String, Option<Vec<SftpFile>>>,
    next: u32,
}

fn status(error: &std::io::Error) -> StatusCode {
    match error.kind() {
        ErrorKind::NotFound => StatusCode::NoSuchFile,
        ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_owned(),
        language_tag: "en-US".to_owned(),
    }
}

impl Files {
    fn path(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    fn file(&mut self, handle: &str) -> Result<&mut File, StatusCode> {
        self.open.get_mut(handle).ok_or(StatusCode::Failure)
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        self.next.to_string()
    }

    fn attrs(&self, path: &str, id: u32) -> Result<Attrs, StatusCode> {
        let metadata = fs::metadata(self.path(path)).map_err(|e| status(&e))?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&metadata),
        })
    }
}

impl russh_sftp::server::Handler for Files {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _: u32, _: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let path = self.path(&filename);
        let file = if flags.contains(OpenFlags::WRITE) {
            OpenOptions::new()
                .write(true)
                .append(flags.contains(OpenFlags::APPEND))
                .truncate(flags.contains(OpenFlags::TRUNCATE))
                .create_new(flags.contains(OpenFlags::EXCLUDE))
                .create(flags.contains(OpenFlags::CREATE))
                .open(path)
        } else {
            File::open(path)
        }
        .map_err(|e| status(&e))?;
        let handle = self.handle();
        self.open.insert(handle.clone(), file);
        Ok(Handle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open.remove(&handle);
        self.dirs.remove(&handle);
        Ok(ok(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let file = self.file(&handle)?;
        let mut data = vec![0; len as usize];
        file.seek(SeekFrom::Start(offset)).map_err(|e| status(&e))?;
        let read = file.read(&mut data).map_err(|e| status(&e))?;
        if read == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(read);
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let file = self.file(&handle)?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| status(&e))?;
        file.write_all(&data).map_err(|e| status(&e))?;
        Ok(ok(id))
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let file = self.file(&handle)?;
        let metadata = file.metadata().map_err(|e| status(&e))?;
        let mut attrs = FileAttributes::from(&metadata);
        // A file starting with `LIAR` reports no size through its handle, like a server
        // whose sizes cannot be trusted.
        let mut start = [0_u8; 4];
        file.seek(SeekFrom::Start(0)).map_err(|e| status(&e))?;
        if file.read_exact(&mut start).is_ok() && &start == b"LIAR" {
            attrs.size = Some(0);
        }
        Ok(Attrs { id, attrs })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.attrs(&path, id)
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.attrs(&path, id)
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(self.path(&path)).map_err(|e| status(&e))? {
            let entry = entry.map_err(|e| status(&e))?;
            let metadata = entry.metadata().map_err(|e| status(&e))?;
            entries.push(SftpFile::new(
                entry.file_name().to_string_lossy(),
                FileAttributes::from(&metadata),
            ));
        }
        let handle = self.handle();
        self.dirs.insert(handle.clone(), Some(entries));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        match self.dirs.get_mut(&handle).and_then(Option::take) {
            Some(files) => Ok(Name { id, files }),
            None => Err(StatusCode::Eof),
        }
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _: FileAttributes,
    ) -> Result<Status, Self::Error> {
        fs::create_dir(self.path(&path)).map_err(|e| status(&e))?;
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        fs::remove_file(self.path(&filename)).map_err(|e| status(&e))?;
        Ok(ok(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let target = self.path(&newpath);
        if target.exists() {
            return Err(StatusCode::Failure);
        }
        fs::rename(self.path(&oldpath), target).map_err(|e| status(&e))?;
        Ok(ok(id))
    }
}
