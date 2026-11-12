//! SMTP delivery with `lettre` (rustls, no OpenSSL).

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use packie_config::{EmailSettings, MailSecurity, Secret};

use crate::SendError;
use crate::ca::Roots;
use crate::content::Content;
use crate::error::SmtpFailure;

/// Time for connecting and for each SMTP command.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Sends `content` to the configured recipients over SMTP.
pub(crate) async fn send(
    settings: &EmailSettings,
    password: Option<&Secret>,
    roots: Roots<'_>,
    content: &Content,
) -> Result<(), SendError> {
    let message = build_message(settings, content)?;
    let transport = transport(settings, password, roots)?;
    transport
        .send(message)
        .await
        .map(|_| ())
        .map_err(|e| SendError::Smtp(SmtpFailure::from(&e)))
}

/// The plain-text message for `content` from the settings' sender to every recipient.
pub(crate) fn build_message(
    settings: &EmailSettings,
    content: &Content,
) -> Result<Message, SendError> {
    let mailbox = |text: &str| text.parse::<Mailbox>().map_err(|_| SendError::Address);
    let mut builder = Message::builder()
        .from(mailbox(&settings.from)?)
        .subject(content.subject())
        .header(ContentType::TEXT_PLAIN);
    for to in &settings.to {
        builder = builder.to(mailbox(to)?);
    }
    builder.body(content.text()).map_err(SendError::MailBuild)
}

/// TLS parameters that trust the bundled Mozilla roots and, in addition, `roots`.
fn tls_parameters(
    host: &str,
    roots: &[ureq::tls::Certificate<'static>],
) -> Result<TlsParameters, lettre::transport::smtp::Error> {
    let mut builder = TlsParameters::builder(host.to_owned());
    for root in roots {
        builder = builder.add_root_certificate(Certificate::from_der(root.der().to_vec())?);
    }
    builder.build_rustls()
}

/// Whether the TLS stack accepts every certificate of a CA bundle as a trust anchor.
pub(crate) fn check_roots(
    roots: &[ureq::tls::Certificate<'static>],
) -> Result<(), lettre::transport::smtp::Error> {
    tls_parameters("localhost", roots).map(|_| ())
}

/// The SMTP transport for the settings, trusting `roots` in addition for TLS modes.
pub(crate) fn transport(
    settings: &EmailSettings,
    password: Option<&Secret>,
    roots: Roots<'_>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, SendError> {
    let port = u16::try_from(settings.port).map_err(|_| SendError::Port)?;
    let host = settings.host.as_str();
    let params = || tls_parameters(host, roots.unwrap_or_default()).map_err(SendError::MailSetup);
    let tls = match settings.security {
        MailSecurity::Starttls => Tls::Required(params()?),
        MailSecurity::Tls => Tls::Wrapper(params()?),
        MailSecurity::Plain => Tls::None,
    };
    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
        .port(port)
        .tls(tls)
        .timeout(Some(TIMEOUT));
    if let (Some(user), Some(password)) = (&settings.username, password) {
        builder = builder.credentials(Credentials::new(user.clone(), password.expose().to_owned()));
    }
    Ok(builder.build())
}
