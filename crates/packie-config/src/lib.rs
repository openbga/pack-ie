//! Configuration of a Positive ACK node, exactly as `docs/spec/configuration.md`
//! defines it.
//!
//! Two documents, both JSON with `snake_case` keys and unknown fields rejected (except
//! inside `tags`):
//!
//! - [`Bootstrap`] (`bootstrap.json`, §3): node identity, paths, IPC, hosting, runtime
//!   file and versions, logging, diagnostics, secrets provider. Read once at start with
//!   [`Bootstrap::load`].
//! - [`RuntimeConfig`] (`runtime.json`, §4–§8): [`EndpointConfig`]s (`kind` and
//!   `settings`), [`ChannelConfig`]s (`bridge` or `message`), alert thresholds and
//!   diagnostics overrides. Connectors carry `protocol` and `settings`
//!   ([`ProtocolSettings`]); filters and mappings stay JSON and are compiled with
//!   `packie-transform` against the protocol models when a document is validated. Ids are
//!   operator-chosen names (`packie_core::Name`).
//!
//! [`RuntimeConfig::parse`] is the only way a runtime document enters the engine: it
//! deserializes with field paths and then [`validate`]s, returning every [`Problem`]
//! (`{severity, code, path, message}`, §9) at once; warnings do not reject. [`ConfigStore`]
//! keeps saved versions as files written by [`write_atomic`] (a new owner-only temporary file,
//! synced and renamed). [`ConfigDiff::between`] tells apply
//! which channels were added, changed (restart), updated (in place), removed or left alone.
//!
//! Secrets are [`SecretRef`] names (`{"secret": name}`), never values.

mod bootstrap;
mod channel;
mod defaults;
mod diff;
mod endpoint;
mod error;
mod notifications;
mod protocol;
pub use protocol::profiles;
mod runtime;
mod secret;
mod secrets;
mod store;
mod tagged;
mod validate;

pub use bootstrap::{
    AuditSettings, BackupSchedule, BackupSettings, Bootstrap, DiagnosticsSettings, Environment,
    HostingSettings, IpcSettings, LogLevelName, LoggingSettings, NodeSettings, PathSettings,
    RuntimeSettings, SecretsProvider, SecretsSettings,
};
pub use channel::{
    BridgeChannel, BufferPolicy, ChannelConfig, ConnectStrategy, Connection, DeliveryOverrides,
    DeliveryPolicy, Destination, EffectiveDelivery, MessageChannel, OverflowAction, Retention,
    RetryPolicy, SourceConnector, Tap, TapFramer, TapRetention,
};
pub use diff::{
    ApplyOutcome, ApplyReport, Change, ChannelChange, ChannelOutcome, ConfigDiff, EndpointChange,
};
pub use endpoint::{
    Cidr, CidrError, EndpointConfig, FlowControl, FolderMode, FolderSettings, KeepAlive,
    OnConsumed, Parity, SerialSettings, SftpAuth, SftpSettings, StopBits, TcpClientSettings,
    TcpListenerSettings, Transport, WriteMode,
};
pub use error::ConfigError;
pub use notifications::{EmailSettings, MailSecurity, NotificationSettings, WebhookSettings};
pub use protocol::{
    AckMode, AstmRole, AstmSettings, AuClass, AuFormat, AuFraming, AuProfile, AuSettings, Charset,
    Column, ColumnType, DelimitedSettings, Hl7Settings, LineEnding, OnExists, ProcessingId,
    ProtocolSettings, RecordMode, ReplyType,
};
pub use runtime::{AlertThresholds, DiagnosticsOverrides, RuntimeConfig, SCHEMA_VERSION};
pub use secret::SecretRef;
pub use secrets::{
    FileSecrets, KEYRING_SERVICE, KeyringSecrets, MapSecrets, Secret, SecretError, SecretResolver,
    secret_store_problem, secrets_for,
};
pub use store::{
    ConfigStore, Verification, VersionInfo, create_private_dir, load_runtime_file,
    persist_replacing, retry_while_busy, write_atomic,
};
pub use validate::{
    Problem, ProblemCode, ProblemSeverity, ValidationErrors, unresolved_secrets, validate,
};
