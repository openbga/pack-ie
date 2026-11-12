//! Webhook delivery: one JSON POST with `ureq` (rustls, blocking, run off the runtime).

use std::time::Duration;

use packie_config::{Secret, WebhookSettings};
use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::SendError;
use crate::ca::Roots;
use crate::content::Content;
use crate::error::HttpFailure;

/// POSTs `content` as JSON to the webhook URL.
pub(crate) async fn send(
    settings: &WebhookSettings,
    bearer: Option<Secret>,
    roots: Roots<'_>,
    content: &Content,
) -> Result<(), SendError> {
    let roots = roots.map(<[Certificate<'static>]>::to_vec);
    let url = settings.url.clone();
    let timeout = Duration::from_secs(u64::from(settings.timeout_seconds));
    let body = content.json().to_string();
    tokio::task::spawn_blocking(move || post(&url, timeout, bearer.as_ref(), roots, &body))
        .await
        .map_err(SendError::HttpTask)?
}

/// One blocking POST; any non-success status is an error.
fn post(
    url: &str,
    timeout: Duration,
    bearer: Option<&Secret>,
    roots: Option<Vec<Certificate<'static>>>,
    body: &str,
) -> Result<(), SendError> {
    // Redirects are not followed: a redirect would send the bearer token elsewhere.
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false);
    if let Some(roots) = roots {
        // ureq offers either its bundled Mozilla roots or a given set, not both: with a
        // `ca_file` the webhook trusts the file's certificates only.
        let tls = TlsConfig::builder()
            .root_certs(RootCerts::from(roots))
            .build();
        config = config.tls_config(tls);
    }
    let agent: ureq::Agent = config.build().into();
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {}", token.expose()));
    }
    let status = request
        .send(body)
        .map_err(|e| SendError::Http(HttpFailure::classify(&e, url.starts_with("https://"))))?
        .status();
    if status.is_success() {
        Ok(())
    } else {
        Err(SendError::HttpStatus(status.as_u16()))
    }
}
