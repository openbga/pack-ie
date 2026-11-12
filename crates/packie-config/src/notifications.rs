//! The bootstrap's `notifications` section (`docs/spec/configuration.md` §3): where alert
//! messages go. Absent targets mean notifications are off.

use std::path::PathBuf;

use packie_core::Severity;
use serde::{Deserialize, Serialize};

use crate::secret::SecretRef;
use crate::validate::{Problem, ProblemCode};

/// Alert notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationSettings {
    /// At most one message per alert (code and scope) in this many minutes, 1–1440.
    /// Raised and cleared both count. Default 15.
    #[serde(default = "fifteen")]
    pub rate_limit_minutes: u32,
    /// A PEM file with the certificates of private certificate authorities, absolute path.
    /// The e-mail relay's and the webhook's TLS certificates may chain to them; the
    /// e-mail relay also still accepts the bundled public roots.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
    /// An SMTP relay.
    #[serde(default)]
    pub email: Option<EmailSettings>,
    /// An HTTP endpoint that receives a JSON POST.
    #[serde(default)]
    pub webhook: Option<WebhookSettings>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            rate_limit_minutes: fifteen(),
            ca_file: None,
            email: None,
            webhook: None,
        }
    }
}

/// How the SMTP connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MailSecurity {
    /// Plain connection upgraded with STARTTLS (usually port 587).
    #[serde(rename = "starttls")]
    Starttls,
    /// TLS from the first byte (usually port 465).
    #[serde(rename = "tls")]
    Tls,
    /// No encryption; only for a relay on this computer.
    #[serde(rename = "none")]
    Plain,
}

/// An SMTP relay and its recipients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailSettings {
    /// Relay host name or address.
    pub host: String,
    /// Relay port, 1–65535.
    pub port: u32,
    /// Default `starttls`; `none` only with `localhost`, `127.0.0.1` or `::1`.
    #[serde(default = "starttls")]
    pub security: MailSecurity,
    /// Login name; set together with `password`.
    #[serde(default)]
    pub username: Option<String>,
    /// Login password, a secret reference; set together with `username`.
    #[serde(default)]
    pub password: Option<SecretRef>,
    /// Sender address.
    pub from: String,
    /// Recipient addresses, at least one.
    pub to: Vec<String>,
    /// Severities that are sent, `warning` and/or `critical`. Default both.
    #[serde(default = "both_severities")]
    pub severities: Vec<Severity>,
}

/// A webhook receiver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookSettings {
    /// `https://…`, or `http://…` for localhost only.
    pub url: String,
    /// Sent as `Authorization: Bearer <secret>`; a secret reference.
    #[serde(default)]
    pub bearer: Option<SecretRef>,
    /// Severities that are sent, `warning` and/or `critical`. Default both.
    #[serde(default = "both_severities")]
    pub severities: Vec<Severity>,
    /// Time for connecting and answering, 1–60. Default 10.
    #[serde(default = "ten")]
    pub timeout_seconds: u32,
}

impl NotificationSettings {
    /// Whether any target is configured.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.email.is_some() || self.webhook.is_some()
    }

    /// The secret references the section uses, with their field paths.
    #[must_use]
    pub fn secret_refs(&self) -> Vec<(&'static str, &SecretRef)> {
        let email = self
            .email
            .as_ref()
            .and_then(|e| e.password.as_ref())
            .map(|s| ("notifications.email.password", s));
        let webhook = self
            .webhook
            .as_ref()
            .and_then(|w| w.bearer.as_ref())
            .map(|s| ("notifications.webhook.bearer", s));
        email.into_iter().chain(webhook).collect()
    }

    /// Every problem of the section, with `notifications.…` paths.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut schema = |path: &str, message: &str| {
            out.push(Problem::error(ProblemCode::Schema, path, message));
        };
        if !(1..=1440).contains(&self.rate_limit_minutes) {
            schema("notifications.rate_limit_minutes", "must be from 1 to 1440");
        }
        if self
            .ca_file
            .as_ref()
            .is_some_and(|f| f.as_os_str().is_empty())
        {
            schema("notifications.ca_file", "must not be empty");
        }
        if let Some(email) = &self.email {
            email.problems(&mut schema);
        }
        if let Some(webhook) = &self.webhook {
            webhook.problems(&mut schema);
        }
        for (path, secret) in self.secret_refs() {
            if secret.is_inline() {
                out.push(Problem::error(
                    ProblemCode::SecretInline,
                    path,
                    "secrets are references: write {\"secret\": \"<name>\"} and store the value \
                     in the secret store",
                ));
            } else if SecretRef::new(secret.name()).is_none() {
                out.push(Problem::error(
                    ProblemCode::Schema,
                    path,
                    "secret names match ^[a-z0-9][a-z0-9_.-]{0,62}$",
                ));
            }
        }
        out
    }
}

impl EmailSettings {
    fn problems(&self, schema: &mut impl FnMut(&str, &str)) {
        if self.host.trim().is_empty() || self.host.contains(char::is_whitespace) {
            schema("notifications.email.host", "must be a host name or address");
        }
        if !(1..=65_535).contains(&self.port) {
            schema("notifications.email.port", "must be from 1 to 65535");
        }
        if self.security == MailSecurity::Plain && !is_local_host(&self.host) {
            schema(
                "notifications.email.security",
                "none is only allowed for a relay on this computer (localhost, 127.0.0.1, ::1)",
            );
        }
        if self.username.as_deref().is_some_and(str::is_empty) {
            schema("notifications.email.username", "must not be empty");
        }
        if self.username.is_some() != self.password.is_some() {
            schema(
                "notifications.email.password",
                "username and password are set together",
            );
        }
        if !is_address(&self.from) {
            schema("notifications.email.from", "must be an e-mail address");
        }
        if self.to.is_empty() {
            schema("notifications.email.to", "needs at least one address");
        }
        for (i, address) in self.to.iter().enumerate() {
            if !is_address(address) {
                schema(
                    &format!("notifications.email.to[{i}]"),
                    "must be an e-mail address",
                );
            }
        }
        severity_problems(&self.severities, "notifications.email.severities", schema);
    }
}

impl WebhookSettings {
    fn problems(&self, schema: &mut impl FnMut(&str, &str)) {
        match url_scheme_host(&self.url) {
            Some(("https", _)) => {}
            Some(("http", host)) if is_local_host(host) => {}
            Some(("http", _)) => schema(
                "notifications.webhook.url",
                "http is only allowed for localhost; use https",
            ),
            _ => schema(
                "notifications.webhook.url",
                "must be an https:// URL (http:// for localhost) without a user name",
            ),
        }
        if !(1..=60).contains(&self.timeout_seconds) {
            schema(
                "notifications.webhook.timeout_seconds",
                "must be from 1 to 60",
            );
        }
        severity_problems(&self.severities, "notifications.webhook.severities", schema);
    }
}

fn severity_problems(severities: &[Severity], path: &str, schema: &mut impl FnMut(&str, &str)) {
    if severities.is_empty() || severities.contains(&Severity::Info) {
        schema(path, "must list warning and/or critical");
    }
}

/// Host names that mean this computer.
fn is_local_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// Roughly an address: one `@`, something on both sides, no spaces or angle brackets.
fn is_address(text: &str) -> bool {
    let mut parts = text.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !text.contains(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
}

/// The scheme and host of an `http(s)` URL; `None` for anything else, a URL with a user
/// name, or one without a host. Brackets of an IPv6 host are kept.
fn url_scheme_host(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme, "http" | "https") || url.contains(char::is_whitespace) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => &authority[..=v6.find(']')? + 1],
        None => authority.split(':').next()?,
    };
    (!host.is_empty()).then_some((scheme, host))
}

const fn fifteen() -> u32 {
    15
}
const fn ten() -> u32 {
    10
}
const fn starttls() -> MailSecurity {
    MailSecurity::Starttls
}
fn both_severities() -> Vec<Severity> {
    vec![Severity::Warning, Severity::Critical]
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn parse(value: &Value) -> NotificationSettings {
        serde_json::from_value(value.clone()).unwrap()
    }

    fn paths(value: &Value) -> Vec<String> {
        parse(value)
            .problems()
            .into_iter()
            .map(|p| p.path)
            .collect()
    }

    fn valid() -> Value {
        json!({
            "rate_limit_minutes": 15,
            "email": {
                "host": "smtp.lab.example", "port": 587, "security": "starttls",
                "username": "packie", "password": {"secret": "smtp-password"},
                "from": "packie@lab.example", "to": ["oncall@lab.example"],
                "severities": ["warning", "critical"]
            },
            "webhook": {
                "url": "https://chat.lab.example/hooks/packie",
                "bearer": {"secret": "webhook-token"},
                "severities": ["critical"], "timeout_seconds": 10
            }
        })
    }

    #[test]
    fn a_complete_section_is_valid_and_an_empty_one_is_off() {
        let settings = parse(&valid());
        assert!(settings.problems().is_empty());
        assert!(settings.is_enabled());
        assert_eq!(settings.secret_refs().len(), 2);
        let empty = parse(&json!({}));
        assert!(empty.problems().is_empty());
        assert!(!empty.is_enabled());
        assert_eq!(empty.rate_limit_minutes, 15);
    }

    #[test]
    fn defaults_apply() {
        let settings = parse(&json!({
            "email": {"host": "localhost", "port": 25, "security": "none",
                      "from": "a@b.example", "to": ["c@d.example"]},
            "webhook": {"url": "http://127.0.0.1:8080/x"}
        }));
        assert!(settings.problems().is_empty());
        let email = settings.email.unwrap();
        assert_eq!(email.severities, [Severity::Warning, Severity::Critical]);
        let webhook = settings.webhook.unwrap();
        assert_eq!(webhook.timeout_seconds, 10);
        assert_eq!(webhook.severities, [Severity::Warning, Severity::Critical]);
        let default_security: EmailSettings =
            serde_json::from_value(json!({"host": "h", "port": 25, "from": "a@b", "to": ["c@d"]}))
                .unwrap();
        assert_eq!(default_security.security, MailSecurity::Starttls);
    }

    #[test]
    fn ca_file_is_a_path() {
        let mut value = valid();
        assert!(parse(&value).ca_file.is_none());
        let absolute = std::env::temp_dir().join("lab-ca.pem");
        value["ca_file"] = json!(absolute);
        assert!(paths(&value).is_empty());
        assert_eq!(parse(&value).ca_file, Some(absolute));
        value["ca_file"] = json!("");
        assert_eq!(paths(&value), ["notifications.ca_file"]);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = valid();
        value["email"]["tls"] = json!(true);
        assert!(serde_json::from_value::<NotificationSettings>(value).is_err());
    }

    #[test]
    fn rate_limit_and_timeout_ranges() {
        let mut value = valid();
        value["rate_limit_minutes"] = json!(0);
        value["webhook"]["timeout_seconds"] = json!(61);
        assert_eq!(
            paths(&value),
            [
                "notifications.rate_limit_minutes",
                "notifications.webhook.timeout_seconds"
            ]
        );
        value["rate_limit_minutes"] = json!(1441);
        value["webhook"]["timeout_seconds"] = json!(0);
        assert_eq!(paths(&value).len(), 2);
    }

    #[test]
    fn email_problems() {
        let mut value = valid();
        value["email"]["host"] = json!(" ");
        value["email"]["port"] = json!(0);
        value["email"]["from"] = json!("nobody");
        value["email"]["to"] = json!(["a@b@c", "ok@lab.example"]);
        value["email"]["severities"] = json!([]);
        assert_eq!(
            paths(&value),
            [
                "notifications.email.host",
                "notifications.email.port",
                "notifications.email.from",
                "notifications.email.to[0]",
                "notifications.email.severities"
            ]
        );
        value["email"]["to"] = json!([]);
        assert!(paths(&value).contains(&"notifications.email.to".to_owned()));
    }

    #[test]
    fn plain_mail_is_for_this_computer_only() {
        let mut value = valid();
        value["email"]["security"] = json!("none");
        assert_eq!(paths(&value), ["notifications.email.security"]);
        for host in ["localhost", "127.0.0.1", "::1"] {
            value["email"]["host"] = json!(host);
            assert!(paths(&value).is_empty(), "{host}");
        }
    }

    #[test]
    fn login_is_a_pair_of_user_and_secret() {
        let mut value = valid();
        value["email"].as_object_mut().unwrap().remove("password");
        assert_eq!(paths(&value), ["notifications.email.password"]);
        value["email"].as_object_mut().unwrap().remove("username");
        assert!(paths(&value).is_empty());
    }

    #[test]
    fn inline_secrets_are_rejected_without_keeping_the_value() {
        let mut value = valid();
        value["email"]["password"] = json!("hunter2");
        value["webhook"]["bearer"] = json!("tok");
        let settings = parse(&value);
        let problems = settings.problems();
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.code == ProblemCode::SecretInline));
        assert!(!format!("{settings:?}").contains("hunter2"));
        value["webhook"]["bearer"] = json!({"secret": "Bad Name"});
        assert_eq!(
            paths(&value).last().unwrap(),
            "notifications.webhook.bearer"
        );
    }

    #[test]
    fn webhook_url_rules() {
        let mut value = valid();
        for (url, ok) in [
            ("https://chat.lab.example/hooks/packie", true),
            ("https://chat.lab.example:8443", true),
            ("http://localhost:9000/x", true),
            ("http://[::1]:9000/x", true),
            ("http://chat.lab.example/x", false),
            ("ftp://chat.lab.example/x", false),
            ("https://user:pw@chat.lab.example/x", false),
            ("https:///x", false),
            ("chat.lab.example", false),
            ("https://chat lab/x", false),
        ] {
            value["webhook"]["url"] = json!(url);
            assert_eq!(paths(&value).is_empty(), ok, "{url}");
        }
    }

    #[test]
    fn info_is_not_a_notification_severity() {
        let mut value = valid();
        value["webhook"]["severities"] = json!(["info"]);
        assert_eq!(paths(&value), ["notifications.webhook.severities"]);
    }
}
