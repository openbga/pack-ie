//! Checks of endpoints (§5) and of the ports they claim.

use std::collections::HashMap;
use std::net::IpAddr;

use super::{Endpoints, ProblemCode, Validator, count};
use crate::endpoint::{
    EndpointConfig, FolderSettings, KeepAlive, SerialSettings, SftpSettings, TcpClientSettings,
    TcpListenerSettings, Transport,
};
use crate::secret::SecretRef;

impl Validator {
    /// Returns the endpoints by id, first definition winning.
    pub(super) fn endpoints<'a>(&mut self, endpoints: &'a [EndpointConfig]) -> Endpoints<'a> {
        let mut by_id = HashMap::new();
        for (i, endpoint) in endpoints.iter().enumerate() {
            let path = format!("endpoints[{i}]");
            if by_id.insert(endpoint.id, endpoint).is_some() {
                self.duplicate(format!("{path}.endpoint_id"), "endpoint id");
            }
            self.label(&path, &endpoint.label);
            self.tags(&path, &endpoint.tags);
            let path = format!("{path}.settings");
            match &endpoint.transport {
                Transport::Serial(s) => self.serial(&path, s),
                Transport::TcpClient(s) => self.tcp_client(&path, s),
                Transport::TcpListener(s) => self.tcp_listener(&path, s),
                Transport::Folder(s) => self.folder(&path, s),
                Transport::Sftp(s) => self.sftp(&path, s),
            }
        }
        by_id
    }

    fn serial(&mut self, path: &str, s: &SerialSettings) {
        self.non_empty(path, "port_name", &s.port_name);
        self.positive(path, "baud_rate", s.baud_rate);
        self.range(path, "data_bits", s.data_bits, 5..=8);
        self.range(path, "read_timeout_ms", s.read_timeout_ms, 1..=60_000);
        self.range(path, "write_timeout_ms", s.write_timeout_ms, 1..=60_000);
        let is_virtual = s.port_name == SerialSettings::VIRTUAL;
        match &s.virtual_link {
            Some(_) if cfg!(windows) => {
                self.schema(
                    format!("{path}.virtual_link"),
                    "virtual serial links need Unix",
                );
            }
            Some(link) if !is_virtual || link.as_os_str().is_empty() => self.schema(
                format!("{path}.virtual_link"),
                "a virtual link needs a path and port_name \"virtual\"",
            ),
            None if is_virtual => {
                self.schema(
                    format!("{path}.port_name"),
                    "\"virtual\" needs a virtual_link path",
                );
            }
            _ => {}
        }
    }

    fn keep_alive(&mut self, path: &str, k: KeepAlive) {
        if k.enabled {
            self.range(path, "keep_alive_time_sec", k.time_seconds, 1..=7200);
            self.range(
                path,
                "keep_alive_interval_sec",
                k.interval_seconds,
                1..=3600,
            );
            self.range(path, "keep_alive_retry_count", k.retry_count, 1..=100);
        }
    }

    fn port(&mut self, path: &str, port: u16) {
        if port == 0 {
            self.schema(format!("{path}.port"), "must be a port from 1 to 65535");
        }
    }

    fn idle(&mut self, path: &str, seconds: u32) {
        if seconds > 604_800 {
            self.schema(
                format!("{path}.idle_timeout_seconds"),
                "must be 0 (off) or up to 604800",
            );
        }
    }

    fn tcp_client(&mut self, path: &str, s: &TcpClientSettings) {
        self.non_empty(path, "host", &s.host);
        self.port(path, s.port);
        self.range(
            path,
            "connect_timeout_ms",
            s.connect_timeout_ms,
            1..=120_000,
        );
        self.idle(path, s.idle_timeout_seconds);
        self.keep_alive(path, s.keep_alive());
    }

    fn tcp_listener(&mut self, path: &str, s: &TcpListenerSettings) {
        self.port(path, s.port);
        self.range(path, "accept_backlog", s.accept_backlog, 1..=1024);
        self.idle(path, s.idle_timeout_seconds);
        self.keep_alive(path, s.keep_alive());
        if !s.bind_address.is_loopback() && s.allowed_peer_cidrs.is_empty() && !s.allow_any_peer {
            self.err(
                ProblemCode::AllowlistRequired,
                format!("{path}.allowed_peer_cidrs"),
                "a listener reachable from other hosts needs allowed_peer_cidrs, \
                 or allow_any_peer set to true",
            );
        }
    }

    /// The names of the files folder and SFTP endpoints select.
    fn file_selection(
        &mut self,
        path: &str,
        pattern: &str,
        ignore_suffixes: &[String],
        processed_directory: &str,
    ) {
        self.non_empty(path, "file_pattern", pattern);
        if pattern.contains(['/', '\\']) {
            self.schema(
                format!("{path}.file_pattern"),
                "matches file names, without directories",
            );
        }
        for (i, suffix) in ignore_suffixes.iter().enumerate() {
            self.non_empty(path, &format!("ignore_suffixes[{i}]"), suffix);
        }
        self.non_empty(path, "processed_directory", processed_directory);
        // Both separators, as the SFTP adapter normalizes them (and Windows reads them).
        let absolute = processed_directory.starts_with(['/', '\\'])
            || processed_directory.as_bytes().get(1) == Some(&b':');
        if absolute
            || processed_directory
                .split(['/', '\\'])
                .any(|part| part == "..")
        {
            self.schema(
                format!("{path}.processed_directory"),
                "must be a directory inside the source directory",
            );
        }
    }

    /// When folder and SFTP endpoints look for files and call them complete.
    fn file_timing(&mut self, path: &str, poll_interval_seconds: u32, stable_seconds: u32) {
        self.range(
            path,
            "poll_interval_seconds",
            poll_interval_seconds,
            1..=86_400,
        );
        self.range(path, "stable_seconds", stable_seconds, 0..=3600);
    }

    fn folder(&mut self, path: &str, s: &FolderSettings) {
        if s.path.as_os_str().is_empty() {
            self.schema(format!("{path}.path"), "must not be empty");
        }
        self.file_selection(
            path,
            &s.file_pattern,
            &s.ignore_suffixes,
            &s.processed_directory,
        );
        self.file_timing(path, s.poll_interval_seconds, s.stable_seconds);
    }

    fn sftp(&mut self, path: &str, s: &SftpSettings) {
        self.non_empty(path, "host", &s.host);
        self.port(path, s.port);
        self.range(path, "username", count(s.username.chars().count()), 1..=255);
        if !s.remote_directory.starts_with('/') {
            self.schema(
                format!("{path}.remote_directory"),
                "must be an absolute remote path",
            );
        }
        if !s.host_key_fingerprint.starts_with("SHA256:") || s.host_key_fingerprint.len() < 10 {
            self.schema(
                format!("{path}.host_key_fingerprint"),
                "must be the server's SHA256:... fingerprint (ssh-keygen -lf)",
            );
        }
        for (field, secret) in s.auth.secret_refs() {
            self.secret(&format!("{path}.auth.{field}"), secret);
        }
        self.file_selection(
            path,
            &s.file_pattern,
            &s.ignore_suffixes,
            &s.processed_directory,
        );
        self.file_timing(path, s.poll_interval_seconds, s.stable_seconds);
        self.range(
            path,
            "connect_timeout_ms",
            s.connect_timeout_ms,
            1..=120_000,
        );
        self.range(
            path,
            "operation_timeout_ms",
            s.operation_timeout_ms,
            1..=600_000,
        );
    }

    fn secret(&mut self, path: &str, secret: &SecretRef) {
        if secret.is_inline() {
            self.err(
                ProblemCode::SecretInline,
                path,
                "secrets are references: write {\"secret\": \"<name>\"} and store the value \
                 in the secret store",
            );
        } else if !SecretRef::is_valid_name(secret.name()) {
            self.schema(path, "secret names match ^[a-z0-9][a-z0-9_.-]{0,62}$");
        }
    }

    /// Two enabled endpoints may not open the same serial port or listen on overlapping
    /// addresses.
    pub(super) fn port_conflicts(&mut self, endpoints: &[EndpointConfig]) {
        let mut serial: HashMap<String, usize> = HashMap::new();
        let mut listeners: Vec<(usize, IpAddr, u16)> = Vec::new();
        for (i, endpoint) in endpoints.iter().enumerate().filter(|(_, e)| e.enabled) {
            match &endpoint.transport {
                Transport::Serial(s) => self.serial_conflict(i, s, &mut serial),
                Transport::TcpListener(s) => self.listener_conflict(i, s, &mut listeners),
                _ => {}
            }
        }
    }

    fn serial_conflict(&mut self, i: usize, s: &SerialSettings, seen: &mut HashMap<String, usize>) {
        // A virtual endpoint is identified by its link path, a physical one by its port.
        let key = match &s.virtual_link {
            Some(link) => format!("link:{}", link.display()),
            None => s.port_name.to_lowercase(),
        };
        if let Some(first) = seen.insert(key, i) {
            self.err(
                ProblemCode::PortConflict,
                format!("endpoints[{i}].settings.port_name"),
                format!("serial port is already used by endpoints[{first}]"),
            );
        }
    }

    fn listener_conflict(
        &mut self,
        i: usize,
        s: &TcpListenerSettings,
        seen: &mut Vec<(usize, IpAddr, u16)>,
    ) {
        let overlaps = |addr: &IpAddr| {
            *addr == s.bind_address || addr.is_unspecified() || s.bind_address.is_unspecified()
        };
        if let Some((first, _, _)) = seen
            .iter()
            .find(|(_, addr, port)| *port == s.port && overlaps(addr))
        {
            self.err(
                ProblemCode::PortConflict,
                format!("endpoints[{i}].settings.port"),
                format!("port is already bound by endpoints[{first}]"),
            );
        }
        seen.push((i, s.bind_address, s.port));
    }
}
