//! Alert notifications (`docs/spec/diagnostics.md` §6.1): a message by e-mail and/or webhook
//! when an alert is raised, changes severity or clears.
//!
//! [`Notifier::alert_changed`] routes an alert to the targets whose `severities` include
//! it, drops it when the same alert (code and scope) already produced a message within
//! `rate_limit_minutes`, and sends in the background. Messages are built only from the
//! alert's catalog fields (see `content`); `details` and message data never leave the
//! engine. Failures are logged without content and tracked per target for
//! [`Notifier::failing`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use packie_config::{MailSecurity, NotificationSettings, Secret, SecretRef, SecretResolver};
use packie_core::{Severity, Timestamp};
use packie_diagnostics::Alert;
use tokio::runtime::Handle;

mod ca;
mod content;
mod email;
mod error;
mod limit;
mod webhook;

#[cfg(test)]
mod tests;

use ca::{CaFile, Roots};
use content::Content;
use error::SendError;
use limit::{AlertKey, RateLimiter};

/// The target name of e-mail delivery.
pub const EMAIL: &str = "email";
/// The target name of webhook delivery.
pub const WEBHOOK: &str = "webhook";

/// The operator-chosen names of the channel and endpoint an alert concerns, shown next to
/// their ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    /// The channel's label.
    pub channel: Option<String>,
    /// The endpoint's label.
    pub endpoint: Option<String>,
}

/// The outcome of a test message to one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetResult {
    /// `email` or `webhook`.
    pub target: &'static str,
    /// Whether the target accepted the message.
    pub ok: bool,
    /// `sent`, or why not.
    pub detail: String,
}

/// A target whose latest attempt failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failing {
    /// `email` or `webhook`.
    pub target: &'static str,
    /// When the current run of failures began (the first failure after the last success).
    pub since: Timestamp,
    /// Why the newest attempt failed.
    pub error: String,
}

/// A delivery method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Email,
    Webhook,
}

impl Target {
    const ALL: [Self; 2] = [Self::Email, Self::Webhook];

    const fn name(self) -> &'static str {
        match self {
            Self::Email => EMAIL,
            Self::Webhook => WEBHOOK,
        }
    }

    /// Whether `settings` configure this target to connect with TLS.
    fn uses_tls(self, settings: &NotificationSettings) -> bool {
        match self {
            Self::Email => settings
                .email
                .as_ref()
                .is_some_and(|e| e.security != MailSecurity::Plain),
            Self::Webhook => settings
                .webhook
                .as_ref()
                .is_some_and(|w| w.url.starts_with("https://")),
        }
    }
}

/// What changes while the notifier runs.
struct State {
    limiter: RateLimiter,
    failing: HashMap<&'static str, Failing>,
}

/// The shared part of a [`Notifier`].
struct Inner {
    settings: NotificationSettings,
    secrets: Arc<dyn SecretResolver>,
    /// `notifications.ca_file`, read at construction; `None` when unset or no target uses TLS.
    ca: Option<CaFile>,
    node: String,
    state: Mutex<State>,
}

/// Sends alert notifications. Cloning is cheap; clones share limits and failure state.
#[derive(Clone)]
pub struct Notifier {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifier")
            .field("node", &self.inner.node)
            .field("email", &self.inner.settings.email.is_some())
            .field("webhook", &self.inner.settings.webhook.is_some())
            .finish_non_exhaustive()
    }
}

impl Notifier {
    /// A notifier for `settings`; `node_name` is the node's display name. Without targets
    /// every call does nothing.
    #[must_use]
    pub fn new(
        settings: NotificationSettings,
        secrets: Arc<dyn SecretResolver>,
        node_name: String,
    ) -> Self {
        let window = Duration::from_secs(u64::from(settings.rate_limit_minutes) * 60);
        let ca = settings
            .ca_file
            .as_deref()
            .filter(|_| Target::ALL.iter().any(|t| t.uses_tls(&settings)))
            .map(CaFile::load);
        Self {
            inner: Arc::new(Inner {
                settings,
                secrets,
                ca,
                node: node_name,
                state: Mutex::new(State {
                    limiter: RateLimiter::new(window),
                    failing: HashMap::new(),
                }),
            }),
        }
    }

    /// An alert was raised, changed severity or cleared. Applies per-target severity
    /// routing and the per-alert rate limit, then sends in the background; the caller is
    /// never blocked beyond queuing. Needs a Tokio runtime to send.
    pub fn alert_changed(&self, alert: &Alert, labels: &Labels) {
        let targets = self.inner.plan(alert, Instant::now());
        if targets.is_empty() {
            return;
        }
        let Ok(runtime) = Handle::try_current() else {
            tracing::warn!(
                event = "notification.failed",
                error_summary = "no async runtime to send on",
            );
            return;
        };
        let content = Arc::new(Content::from_alert(&self.inner.node, alert).labelled(labels));
        for target in targets {
            let inner = Arc::clone(&self.inner);
            let content = Arc::clone(&content);
            runtime.spawn(async move {
                let _ = inner.deliver(target, &content).await;
            });
        }
    }

    /// Sends a test message to every configured target now, one result per target.
    pub async fn test(&self) -> Vec<TargetResult> {
        let content = Content::test(&self.inner.node);
        let mut results = Vec::new();
        for target in self.inner.targets() {
            let result = self.inner.deliver(target, &content).await;
            results.push(TargetResult {
                target: target.name(),
                ok: result.is_ok(),
                detail: match result {
                    Ok(()) => "sent".to_owned(),
                    Err(e) => e.summary(),
                },
            });
        }
        results
    }

    /// The targets whose latest attempt failed, for the `notifications_failing` alert. A
    /// target leaves the list when a later send to it succeeds.
    #[must_use]
    pub fn failing(&self) -> Vec<Failing> {
        let state = self.inner.lock();
        let mut failing: Vec<Failing> = state.failing.values().cloned().collect();
        failing.sort_by_key(|f| f.target);
        failing
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The severities `target` sends; `None` when it is not configured.
    fn severities(&self, target: Target) -> Option<&[Severity]> {
        match target {
            Target::Email => self
                .settings
                .email
                .as_ref()
                .map(|e| e.severities.as_slice()),
            Target::Webhook => self
                .settings
                .webhook
                .as_ref()
                .map(|w| w.severities.as_slice()),
        }
    }

    fn targets(&self) -> Vec<Target> {
        Target::ALL
            .into_iter()
            .filter(|t| self.severities(*t).is_some())
            .collect()
    }

    /// The targets that should send this event now: severity routing first, then the rate
    /// limit (an event no target wants does not use up the alert's window).
    fn plan(&self, alert: &Alert, now: Instant) -> Vec<Target> {
        let targets: Vec<Target> = self
            .targets()
            .into_iter()
            .filter(|t| {
                self.severities(*t)
                    .is_some_and(|s| s.contains(&alert.severity))
            })
            .collect();
        if targets.is_empty() {
            return targets;
        }
        if self.lock().limiter.allow(AlertKey::of(alert), now) {
            return targets;
        }
        tracing::debug!(
            event = "notification.suppressed",
            alert_code = %Content::from_alert(&self.node, alert).code,
            channel = alert.channel_id.map(|id| id.to_string()),
            endpoint = alert.endpoint_id.map(|id| id.to_string()),
            "within the rate limit window",
        );
        Vec::new()
    }

    /// The value of an optional secret reference, looked up off the runtime.
    async fn resolve(&self, secret: Option<&SecretRef>) -> Result<Option<Secret>, SendError> {
        let Some(secret) = secret else {
            return Ok(None);
        };
        let (secrets, secret) = (Arc::clone(&self.secrets), secret.clone());
        let value = tokio::task::spawn_blocking(move || secrets.resolve(&secret))
            .await
            .map_err(SendError::SecretTask)??;
        Ok(Some(value))
    }

    /// The private CAs to trust for `target` when it uses TLS; the error of an unusable CA
    /// file when it does.
    fn roots(&self, target: Target) -> Result<Roots<'_>, SendError> {
        match &self.ca {
            Some(ca) if target.uses_tls(&self.settings) => ca.certs().map(Some),
            _ => Ok(None),
        }
    }

    async fn send(&self, target: Target, content: &Content) -> Result<(), SendError> {
        let roots = self.roots(target)?;
        match target {
            Target::Email => {
                let settings = self
                    .settings
                    .email
                    .as_ref()
                    .ok_or(SendError::NotConfigured)?;
                let password = self.resolve(settings.password.as_ref()).await?;
                email::send(settings, password.as_ref(), roots, content).await
            }
            Target::Webhook => {
                let settings = self
                    .settings
                    .webhook
                    .as_ref()
                    .ok_or(SendError::NotConfigured)?;
                let bearer = self.resolve(settings.bearer.as_ref()).await?;
                webhook::send(settings, bearer, roots, content).await
            }
        }
    }

    /// Sends to one target and records the outcome in the log and the failure state.
    async fn deliver(&self, target: Target, content: &Content) -> Result<(), SendError> {
        let result = self.send(target, content).await;
        let mut state = self.lock();
        match &result {
            Ok(()) => {
                state.failing.remove(target.name());
                tracing::debug!(
                    event = "notification.sent",
                    target = target.name(),
                    alert_code = %content.code,
                    channel = content.channel_id.as_deref(),
                    endpoint = content.endpoint_id.as_deref(),
                );
            }
            Err(e) => {
                let error = e.summary();
                tracing::warn!(
                    event = "notification.failed",
                    target = target.name(),
                    alert_code = %content.code,
                    channel = content.channel_id.as_deref(),
                    endpoint = content.endpoint_id.as_deref(),
                    error_summary = %error,
                );
                state
                    .failing
                    .entry(target.name())
                    .or_insert_with(|| Failing {
                        target: target.name(),
                        since: Timestamp::now(),
                        error: String::new(),
                    })
                    .error = error;
            }
        }
        result
    }
}
