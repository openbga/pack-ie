//! Checks of protocol settings (§7) and of which endpoint kinds each protocol can use.

use std::collections::HashSet;

use packie_core::EndpointKind;

use super::{MAX_MESSAGE, ProblemCode, Role, Validator};
use crate::protocol::{
    AckMode, AstmSettings, AuFormat, AuSettings, ColumnType, DelimitedSettings, Hl7Settings,
    ProtocolSettings, RecordMode,
};

impl Validator {
    /// The endpoint kinds each protocol can use (§6.4).
    pub(super) fn protocol_fits(
        &mut self,
        path: &str,
        protocol: &ProtocolSettings,
        kind: EndpointKind,
    ) {
        let (fits, needs) = match protocol {
            ProtocolSettings::Astm(_) | ProtocolSettings::Au(_) => (
                kind.is_stream(),
                "a serial, tcp_client or tcp_listener endpoint",
            ),
            ProtocolSettings::Hl7(_) => (
                matches!(kind, EndpointKind::TcpClient | EndpointKind::TcpListener),
                "a tcp_client or tcp_listener endpoint",
            ),
            ProtocolSettings::Delimited(_) => (
                matches!(kind, EndpointKind::Folder | EndpointKind::Sftp),
                "a folder or sftp endpoint",
            ),
            ProtocolSettings::Raw => return,
        };
        if !fits {
            self.err(
                ProblemCode::EndpointKindNotAllowed,
                format!("{path}.protocol"),
                format!("{} needs {needs}", protocol.kind()),
            );
        }
    }

    pub(super) fn protocol(&mut self, path: &str, protocol: &ProtocolSettings, role: Role) {
        let settings = format!("{path}.settings");
        match protocol {
            ProtocolSettings::Astm(s) => self.astm(&settings, s),
            ProtocolSettings::Au(s) => self.au(&settings, s),
            ProtocolSettings::Hl7(s) => self.hl7(&settings, s, role),
            ProtocolSettings::Delimited(s) => self.delimited(&settings, s, role),
            ProtocolSettings::Raw => self.schema(
                format!("{path}.protocol"),
                "raw is only for bridge taps; use astm, au, hl7_mllp or delimited_file",
            ),
        }
    }

    fn astm(&mut self, path: &str, s: &AstmSettings) {
        self.ranges(
            path,
            [
                ("ack_timeout_ms", s.ack_timeout_ms.into(), 1000..=120_000),
                (
                    "receive_timeout_ms",
                    s.receive_timeout_ms.into(),
                    1000..=300_000,
                ),
                ("max_frame_retries", s.max_frame_retries.into(), 0..=20),
                ("max_frame_chars", s.max_frame_chars.into(), 64..=64_000),
                ("enq_attempts", s.enq_attempts.into(), 1..=10),
                ("busy_retry_ms", s.busy_retry_ms.into(), 1000..=60_000),
                (
                    "contention_wait_ms",
                    s.contention_wait_ms.into(),
                    1000..=60_000,
                ),
                ("max_message_bytes", s.max_message_bytes, 1024..=MAX_MESSAGE),
            ],
        );
        if s.sample_id_position().is_none() {
            self.schema(
                format!("{path}.sample_id_path"),
                "must be O.<field> or O.<field>.<component>, field 3 to 99, component 1 to 99",
            );
        }
    }

    fn au(&mut self, path: &str, s: &AuSettings) {
        let control = |bytes: &[u8]| {
            (1..=2).contains(&bytes.len())
                && bytes
                    .iter()
                    .all(|b| (1..=31).contains(b) && !matches!(b, 6 | 21))
        };
        let framing = &s.framing;
        for (field, bytes) in [("start", &framing.start), ("end", &framing.end)] {
            if !control(bytes) {
                self.schema(
                    format!("{path}.framing.{field}"),
                    "must be 1 or 2 control bytes (1-31), not ACK (6) or NAK (21)",
                );
            }
        }
        if framing.use_etb && framing.end != [3] {
            self.schema(
                format!("{path}.framing.use_etb"),
                "only allowed when end is [3] (ETX)",
            );
        }
        if !matches!(framing.max_wire_len, 256 | 512 | 1024) {
            self.schema(
                format!("{path}.framing.max_wire_len"),
                "must be 256, 512 or 1024",
            );
        }
        self.au_format(&format!("{path}.format"), &s.format);
        for (field, value) in [
            ("frame_timeout_ms", s.frame_timeout_ms),
            ("block_timeout_ms", s.block_timeout_ms),
        ] {
            if !(100..=9900).contains(&value) || value % 100 != 0 {
                self.schema(
                    format!("{path}.{field}"),
                    "must be 100 to 9900 in steps of 100",
                );
            }
        }
    }

    fn au_format(&mut self, path: &str, f: &AuFormat) {
        if !matches!(f.rack_digits, 0 | 4 | 5) {
            self.schema(format!("{path}.rack_digits"), "must be 0, 4 or 5");
        }
        self.range(path, "sample_id_digits", f.sample_id_digits, 1..=20);
        if f.patient_info_digits.iter().any(|n| *n > 20) {
            self.schema(
                format!("{path}.patient_info_digits"),
                "each must be from 0 to 20",
            );
        }
        if !matches!(f.result_digits, 6 | 9) {
            self.schema(format!("{path}.result_digits"), "must be 6 or 9");
        }
        if !matches!(f.test_code_digits, 2 | 3) {
            self.schema(format!("{path}.test_code_digits"), "must be 2 or 3");
        }
    }

    fn hl7(&mut self, path: &str, s: &Hl7Settings, role: Role) {
        if s.ack_mode == AckMode::None && role == Role::Source {
            self.schema(
                format!("{path}.ack_mode"),
                "a source acknowledges: original or enhanced",
            );
        }
        if !hl7_version_ok(&s.version) {
            self.schema(format!("{path}.version"), "must be 2.x or 2.x.y");
        }
        for (field, value) in [
            ("sending_application", &s.sending_application),
            ("sending_facility", &s.sending_facility),
            ("receiving_application", &s.receiving_application),
            ("receiving_facility", &s.receiving_facility),
        ] {
            if value.chars().count() > 180 {
                self.schema(format!("{path}.{field}"), "must be at most 180 characters");
            }
        }
        self.range(
            path,
            "max_message_bytes",
            s.max_message_bytes,
            1024..=MAX_MESSAGE,
        );
        self.range(path, "frame_timeout_ms", s.frame_timeout_ms, 1000..=300_000);
        let types = &s.ack_message_types;
        if types.is_empty() || types.len() > 8 {
            self.schema(
                format!("{path}.ack_message_types"),
                "must list 1 to 8 message types",
            );
        }
        for (i, kind) in types.iter().enumerate() {
            if !message_type_ok(kind) {
                self.schema(
                    format!("{path}.ack_message_types[{i}]"),
                    "must be three capital letters or digits, starting with a letter",
                );
            }
        }
    }

    fn delimited(&mut self, path: &str, s: &DelimitedSettings, role: Role) {
        if !matches!(s.delimiter.as_str(), "," | ";" | "|" | "\t") {
            self.schema(format!("{path}.delimiter"), "must be , ; | or a tab");
        }
        if s.quote.as_ref().is_some_and(|q| q.chars().count() != 1) {
            self.schema(format!("{path}.quote"), "must be one character, or null");
        }
        self.range(path, "skip_lines", s.skip_lines, 0..=1000);
        self.columns(path, s);
        self.range(
            path,
            "max_file_bytes",
            s.max_file_bytes,
            1024..=1_073_741_824,
        );
        let name = format!("{path}.file_name");
        match (&s.file_name, role) {
            (None, Role::Destination) => {
                self.schema(name, "a destination needs a file name template");
            }
            // A leading dot would name a hidden file: readers skip those and the
            // transport refuses to write them.
            (Some(file), _)
                if file.trim().is_empty()
                    || file.contains(['/', '\\'])
                    || file.starts_with('.') =>
            {
                self.schema(
                    name,
                    "must be a file name, without directories and not starting with a dot",
                );
            }
            _ => {}
        }
    }

    fn columns(&mut self, path: &str, s: &DelimitedSettings) {
        if s.columns.is_empty() {
            self.schema(format!("{path}.columns"), "must list at least one column");
        }
        let mut names = HashSet::new();
        for (i, column) in s.columns.iter().enumerate() {
            let at = format!("{path}.columns[{i}]");
            if !column_name_ok(&column.name) {
                self.schema(format!("{at}.name"), "must match ^[a-z][a-z0-9_]{0,40}$");
            } else if !names.insert(column.name.as_str()) {
                self.duplicate(format!("{at}.name"), "column name");
            }
            if column.position == Some(0) {
                self.schema(format!("{at}.position"), "positions start at 1");
            }
            let dated = matches!(column.column_type, ColumnType::Date | ColumnType::Datetime);
            if dated && column.format.as_deref().is_none_or(|f| f.trim().is_empty()) {
                self.schema(
                    format!("{at}.format"),
                    "date and datetime columns need a format",
                );
            }
        }
        let group_by = format!("{path}.group_by");
        match (s.record_mode, &s.group_by) {
            (RecordMode::Grouped, None) => {
                self.schema(group_by, "grouped records need a group_by column");
            }
            (RecordMode::Grouped, Some(column)) if !names.contains(column.as_str()) => {
                self.schema(group_by, "must name one of the columns");
            }
            (RecordMode::OnePerLine, Some(_)) => {
                self.schema(group_by, "only used with record_mode grouped");
            }
            _ => {}
        }
    }
}

fn hl7_version_ok(version: &str) -> bool {
    version.strip_prefix("2.").is_some_and(|rest| {
        let parts: Vec<&str> = rest.split('.').collect();
        (1..=2).contains(&parts.len())
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn message_type_ok(kind: &str) -> bool {
    kind.len() == 3
        && kind.starts_with(|c: char| c.is_ascii_uppercase())
        && kind
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn column_name_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && name.len() <= 41
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
