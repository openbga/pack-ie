//! What the operator can do, verbatim from the alert catalog in
//! `docs/spec/diagnostics.md` §6.

pub(super) const ENDPOINT_DOWN: &str = "Check that the device or remote system is on and reachable (cable \
    or network). The engine keeps retrying; the alert clears once the connection is stable.";
pub(super) const ENDPOINT_ERROR: &str = "Fix the cause in the last error (permissions, credentials or host \
    key). The engine retries at its longest interval and recovers on its own once it is fixed.";
pub(super) const NO_PEER: &str = "Check that the instrument is on, cabled or networked, and set to connect \
    to this channel's listener.";
pub(super) const BUFFER_PRESSURE: &str = "The receiving side is slower than the sender, so the engine holds \
    the sender back; no data is lost. Check that the receiving system is processing.";
pub(super) const STORE_UNAVAILABLE: &str = "Check free disk space and the permissions of the state \
    directory. Until storage works, incoming messages are not acknowledged, so senders keep \
    them and retry.";
pub(super) const DISK_LOW: &str = "Free disk space on the state volume or shorten retention. Messages \
    with unresolved deliveries are never purged automatically.";
pub(super) const NEEDS_ATTENTION: &str = "Open Messages filtered to this destination. Rejected or failed: \
    fix the cause, then resend. Timed out or outcome unknown: first check whether the \
    destination received the message, then resend or close with a reason.";
pub(super) const TRANSFORM_FAILED: &str = "Correct the mapping (each affected delivery names the failing \
    rule and path), apply the configuration, then resend the affected messages.";
pub(super) const BACKLOG: &str = "Check the destination endpoint and the receiving system. Messages are \
    safe in the store and are sent in order once the destination accepts them again.";
pub(super) const INVALID_MESSAGES: &str = "The sender produces messages the engine cannot accept. Check \
    its protocol settings (framing, character set, version). The originals are kept in \
    Messages with the reason.";
pub(super) const CONFIG_REJECTED: &str = "Validate the document (Settings → Versions, or `packie config \
    validate <file>`), fix the errors and apply again. The previous configuration stays active.";
pub(super) const AUDIT_UNAVAILABLE: &str = "Check free disk space and the permissions of the audit \
    directory (<state>/audit). Until the audit log can be written, commands that are audited \
    (configuration changes, channel commands, resends, content views) are refused; message \
    traffic continues.";
pub(super) const BACKUP_STALE: &str = "Check the backup directory (free space, permissions, mounted) and \
    the last backup error (Settings → Backup or `packie backup status`), then run `packie backup \
    create`. The alert clears after the next successful backup.";
pub(super) const NOTIFICATIONS_FAILING: &str = "Check the notification settings (host, port, \
    security, credentials secret, webhook address) and that this computer can reach the target, \
    then send a test message (`packie notify test`). The alert clears after the next successful \
    delivery to every target.";
pub(super) const CONFIG_APPLY_FAILED: &str = "The channel's previous definition is still running. Fix the \
    error in the summary and apply again.";
