//! Why a message was not delivered.

use std::path::PathBuf;
use std::sync::Arc;

use packie_config::SecretError;

use crate::ca::CaError;
use crate::content::blank_controls;

/// Longest error text kept for logs and [`Failing`](crate::Failing).
const ERROR_LIMIT: usize = 200;

/// Why a message was not delivered. Texts name settings and servers, never secret values,
/// message content or anything a peer sent: transport errors are reduced to fixed
/// categories (and a numeric status) the moment they occur, so a relay or receiver cannot
/// put its own text into logs, snapshots or later notifications.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SendError {
    /// A secret the target needs cannot be resolved.
    #[error("{0}")]
    Secret(#[from] SecretError),
    /// The background secret lookup did not finish.
    #[error("the secret lookup failed: {0}")]
    SecretTask(#[source] tokio::task::JoinError),
    /// The target has no settings.
    #[error("the target is not configured")]
    NotConfigured,
    /// `notifications.ca_file` is unusable.
    #[error("CA file {} {reason}", path.display())]
    CaFile {
        path: PathBuf,
        #[source]
        reason: Arc<CaError>,
    },
    /// A sender or recipient address does not parse.
    #[error("an e-mail address in the settings is invalid")]
    Address,
    /// The SMTP port does not fit a port number.
    #[error("the SMTP port is out of range")]
    Port,
    /// The mail transport or its TLS settings cannot be built.
    #[error("{0}")]
    MailSetup(#[source] lettre::transport::smtp::Error),
    /// The mail cannot be assembled from the settings.
    #[error("{0}")]
    MailBuild(#[source] lettre::error::Error),
    /// The SMTP exchange failed.
    #[error("{0}")]
    Smtp(SmtpFailure),
    /// The background webhook request did not finish.
    #[error("the send task failed: {0}")]
    HttpTask(#[source] tokio::task::JoinError),
    /// The webhook request failed before a status came back.
    #[error("{0}")]
    Http(HttpFailure),
    /// The webhook receiver answered with a non-success status.
    #[error("the receiver answered HTTP {0}")]
    HttpStatus(u16),
}

/// A failed SMTP exchange as a fixed category and, for a negative reply, its numeric code.
/// Holds nothing the relay sent except that code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SmtpFailure {
    #[error("SMTP relay refused the message ({0})")]
    Refused(u16),
    #[error("SMTP relay refused the credentials ({0})")]
    AuthRefused(u16),
    #[error("SMTP relay deferred the message ({0})")]
    Deferred(u16),
    #[error("TLS handshake with the SMTP relay failed")]
    Tls,
    #[error("timed out talking to the SMTP relay")]
    Timeout,
    #[error("cannot connect to the SMTP relay")]
    Unreachable,
    #[error("the SMTP relay sent a reply that could not be read")]
    BadReply,
    #[error("SMTP error")]
    Other,
}

/// Reply codes that mean authentication was refused or is required (RFC 4954).
const AUTH_CODES: [u16; 4] = [530, 534, 535, 538];

impl From<&lettre::transport::smtp::Error> for SmtpFailure {
    /// Classifies by kind and status only; the error's text (which holds the relay's reply)
    /// is never read.
    fn from(error: &lettre::transport::smtp::Error) -> Self {
        if let Some(code) = error.status().map(u16::from) {
            return if error.is_transient() {
                Self::Deferred(code)
            } else if AUTH_CODES.contains(&code) {
                Self::AuthRefused(code)
            } else {
                Self::Refused(code)
            };
        }
        if error.is_tls() {
            Self::Tls
        } else if error.is_timeout() {
            Self::Timeout
        } else if error.is_response() {
            Self::BadReply
        } else if error.is_client() || error.is_transport_shutdown() {
            Self::Other
        } else {
            // The remaining kinds are connection and network failures.
            Self::Unreachable
        }
    }
}

/// A failed webhook request (no status came back) as a fixed category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum HttpFailure {
    #[error("cannot connect to the webhook receiver")]
    Connect,
    #[error("cannot resolve the webhook host name")]
    Dns,
    #[error("timed out talking to the webhook receiver")]
    Timeout,
    #[error("TLS handshake with the webhook receiver failed")]
    Tls,
    #[error("network error talking to the webhook receiver")]
    Io,
    #[error("the webhook receiver sent a reply that could not be read")]
    BadReply,
    #[error("the webhook receiver answered with a redirect, which is not followed")]
    Redirect,
    #[error("the webhook address or request is invalid")]
    BadRequest,
    #[error("webhook request failed")]
    Other,
}

impl HttpFailure {
    /// Classifies by error variant (and I/O kind) only; the error's text is never read.
    /// `tls` is whether the URL is `https`: the TLS stack reports handshake failures as
    /// invalid-data I/O errors.
    pub(crate) fn classify(error: &ureq::Error, tls: bool) -> Self {
        use ureq::Error as E;
        match error {
            E::Timeout(_) => Self::Timeout,
            E::HostNotFound => Self::Dns,
            E::ConnectionFailed => Self::Connect,
            E::Tls(_) | E::Rustls(_) => Self::Tls,
            E::Io(io) => Self::from_io(io.kind(), tls),
            E::Protocol(_) | E::LargeResponseHeader(..) => Self::BadReply,
            E::TooManyRedirects | E::RedirectFailed => Self::Redirect,
            E::Http(_)
            | E::BadUri(_)
            | E::InvalidProxyUrl
            | E::RequireHttpsOnly(_)
            | E::BodyExceedsLimit(_) => Self::BadRequest,
            _ => Self::Other,
        }
    }

    /// An I/O failure by kind: no connection at all is `Connect`; reply bytes that arrived
    /// but cannot be used (malformed, or cut off by the end of the connection) are
    /// `BadReply`; a connection lost while the request is still being written (reset,
    /// broken pipe: no reply was read) is `Io`.
    fn from_io(kind: std::io::ErrorKind, tls: bool) -> Self {
        use std::io::ErrorKind as K;
        match kind {
            K::TimedOut | K::WouldBlock => Self::Timeout,
            K::ConnectionRefused
            | K::NotConnected
            | K::AddrNotAvailable
            | K::HostUnreachable
            | K::NetworkUnreachable => Self::Connect,
            K::InvalidData if tls => Self::Tls,
            K::InvalidData | K::UnexpectedEof => Self::BadReply,
            _ => Self::Io,
        }
    }
}

impl SendError {
    /// One short line for logs and the alert.
    pub(crate) fn summary(&self) -> String {
        let line: String = blank_controls(&self.to_string())
            .chars()
            .take(ERROR_LIMIT)
            .collect();
        line.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind as K;

    use super::HttpFailure;

    /// Each I/O kind has one fixed category, the same on every operating system.
    #[test]
    fn io_failures_have_one_category_each() {
        for (kind, tls, expected) in [
            (K::TimedOut, false, HttpFailure::Timeout),
            (K::ConnectionRefused, false, HttpFailure::Connect),
            (K::HostUnreachable, false, HttpFailure::Connect),
            (K::ConnectionReset, false, HttpFailure::Io),
            (K::BrokenPipe, false, HttpFailure::Io),
            (K::ConnectionAborted, false, HttpFailure::Io),
            (K::UnexpectedEof, false, HttpFailure::BadReply),
            (K::InvalidData, false, HttpFailure::BadReply),
            (K::InvalidData, true, HttpFailure::Tls),
        ] {
            assert_eq!(
                HttpFailure::from_io(kind, tls),
                expected,
                "{kind:?} tls={tls}"
            );
        }
    }
}
