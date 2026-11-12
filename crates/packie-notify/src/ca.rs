//! The private certificate authorities of `notifications.ca_file`: a PEM bundle read once
//! when the notifier is built. A file that cannot be used is kept as its reason so every
//! TLS send fails naming the file (the `notifications_failing` alert shows it).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ureq::tls::{Certificate, PemItem};

use crate::SendError;
use crate::email;

/// The private CAs of a target that uses TLS; `None` when it trusts the bundled roots only.
pub(crate) type Roots<'a> = Option<&'a [Certificate<'static>]>;

/// Why a CA file cannot be used. Never includes certificate contents.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CaError {
    #[error("cannot be read: {0}")]
    Read(#[from] std::io::Error),
    #[error("is not valid PEM: {0}")]
    Pem(#[from] ureq::Error),
    #[error("holds no certificate")]
    Empty,
    #[error("holds a certificate that cannot be trusted: {0}")]
    Rejected(#[source] lettre::transport::smtp::Error),
}

/// The certificates of one CA file, or the reason they are unusable.
#[derive(Debug)]
pub(crate) struct CaFile {
    path: PathBuf,
    certs: Result<Vec<Certificate<'static>>, Arc<CaError>>,
}

impl CaFile {
    /// Reads and checks `path`; an unusable file is logged once as `notification.ca_invalid`
    /// and remembered.
    pub(crate) fn load(path: &Path) -> Self {
        let certs = read(path).map_err(Arc::new);
        if let Err(reason) = &certs {
            tracing::warn!(
                event = "notification.ca_invalid",
                ca_file = %path.display(),
                error_summary = %reason,
                "the notification CA file cannot be used; TLS sends will fail",
            );
        }
        Self {
            path: path.to_owned(),
            certs,
        }
    }

    /// The certificates to trust, or the error every TLS send reports.
    pub(crate) fn certs(&self) -> Result<&[Certificate<'static>], SendError> {
        self.certs.as_deref().map_err(|reason| SendError::CaFile {
            path: self.path.clone(),
            reason: Arc::clone(reason),
        })
    }
}

fn read(path: &Path) -> Result<Vec<Certificate<'static>>, CaError> {
    let pem = std::fs::read(path)?;
    let certs = parse(&pem)?;
    email::check_roots(&certs).map_err(CaError::Rejected)?;
    Ok(certs)
}

/// Every certificate of a PEM bundle; other PEM sections (keys) are skipped.
fn parse(pem: &[u8]) -> Result<Vec<Certificate<'static>>, CaError> {
    let mut certs = Vec::new();
    for item in ureq::tls::parse_pem(pem) {
        if let PemItem::Certificate(cert) = item? {
            certs.push(cert);
        }
    }
    if certs.is_empty() {
        return Err(CaError::Empty);
    }
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// Two synthetic self-signed CA certificates (test only, generated with openssl, valid
    /// for 100 years).
    const CA_A: &str = "-----BEGIN CERTIFICATE-----
MIIBtDCCAVqgAwIBAgITEv5Zq+Zgnt4d4bjJOVy1hiUomDAKBggqhkjOPQQDAjAn
MRYwFAYDVQQDDA1WTFggVGVzdCBDQSBhMQ0wCwYDVQQKDARUZXN0MCAXDTI2MTEw
NzExMDgzMVoYDzIxMjYxMDE0MTEwODMxWjAnMRYwFAYDVQQDDA1WTFggVGVzdCBD
QSBhMQ0wCwYDVQQKDARUZXN0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEJvhj
PsoyqUdKmbPmXMWJJUZzP7vy2PHpKn0OiaGU1iSxYhN+EHMnWx03M+0od2vWAlXf
uw5TnCKbGeGjRBvBdKNjMGEwHQYDVR0OBBYEFB8lIVf8AzK9qeCc95LiabFFRlKe
MB8GA1UdIwQYMBaAFB8lIVf8AzK9qeCc95LiabFFRlKeMA8GA1UdEwEB/wQFMAMB
Af8wDgYDVR0PAQH/BAQDAgEGMAoGCCqGSM49BAMCA0gAMEUCIQDxtMKRkfhWC+Sg
N4Zq8Ps8KasFcPiIRTIk1xhwGAmu7wIgIzFIvJjwQn2YZBW987JZiDCQldBJEA9A
e9wYaCItfzk=
-----END CERTIFICATE-----
";
    const CA_B: &str = "-----BEGIN CERTIFICATE-----
MIIBtTCCAVugAwIBAgIUDPe977xSSJraszLAEQxBAGrKBoUwCgYIKoZIzj0EAwIw
JzEWMBQGA1UEAwwNVkxYIFRlc3QgQ0EgYjENMAsGA1UECgwEVGVzdDAgFw0yNjEx
MDcxMTA4MzFaGA8yMTI2MTAxNDExMDgzMVowJzEWMBQGA1UEAwwNVkxYIFRlc3Qg
Q0EgYjENMAsGA1UECgwEVGVzdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABOYi
GZciKZgI7vGBn2WLSabD7YMw4tb73QpBOgwSIbCGeobmp+tBSgwkJTwomhPkMNRK
40md0z6hnUF+idg7X0ejYzBhMB0GA1UdDgQWBBTP0u4+rf2689LscrUvU+ewuo8I
wjAfBgNVHSMEGDAWgBTP0u4+rf2689LscrUvU+ewuo8IwjAPBgNVHRMBAf8EBTAD
AQH/MA4GA1UdDwEB/wQEAwIBBjAKBggqhkjOPQQDAgNIADBFAiABH/WUx5o9HTmU
poIho57LnM9EOQFcFBI1ETYjGMJOmwIhAMRrYmJCdcdGCgo4PJ8uA4aUxl8Wvo7H
7jAvAI8RHlyv
-----END CERTIFICATE-----
";

    /// A file in a fresh temporary directory, removed on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(content: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "packie-notify-ca-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("ca.pem");
            std::fs::write(&path, content).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    /// The error of loading a CA file holding `content`.
    fn unusable(content: &str) -> String {
        let file = TempFile::new(content);
        CaFile::load(&file.0).certs().unwrap_err().to_string()
    }

    #[test]
    fn a_bundle_yields_every_certificate() {
        let file = TempFile::new(&format!("# lab CAs\n{CA_A}\n{CA_B}"));
        let ca = CaFile::load(&file.0);
        assert_eq!(ca.certs().unwrap().len(), 2);
        let one = TempFile::new(CA_A);
        assert_eq!(CaFile::load(&one.0).certs().unwrap().len(), 1);
    }

    #[test]
    fn a_missing_file_names_itself_and_the_reason() {
        let missing = std::env::temp_dir().join("packie-notify-ca-does-not-exist.pem");
        let error = CaFile::load(&missing).certs().unwrap_err().to_string();
        assert!(error.starts_with(&format!("CA file {} cannot be read", missing.display())));
    }

    #[test]
    fn an_unusable_file_gives_its_reason_and_never_certificate_text() {
        let garbage = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        let torn = format!("{CA_A}-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n");
        for (content, reason) in [
            ("not a certificate\n", "holds no certificate"),
            ("", "holds no certificate"),
            (garbage, "cannot be trusted"),
            (&torn, "not valid PEM"),
        ] {
            let text = unusable(content);
            assert!(text.contains(reason), "{text}");
            assert!(!text.contains("MIIB"), "{text}");
        }
    }

    #[tokio::test]
    async fn a_valid_file_lets_sends_reach_the_network() {
        let file = TempFile::new(&format!("{CA_A}{CA_B}"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let settings = serde_json::from_value(serde_json::json!({
            "ca_file": file.0,
            "email": {"host": "127.0.0.1", "port": port, "security": "tls",
                      "from": "a@b.example", "to": ["c@d.example"]},
            "webhook": {"url": format!("https://127.0.0.1:{port}/x"), "timeout_seconds": 5}
        }))
        .unwrap();
        let secrets = packie_config::MapSecrets::new(std::collections::HashMap::new());
        let notifier = crate::Notifier::new(settings, std::sync::Arc::new(secrets), "n".into());
        let results = notifier.test().await;
        assert_eq!(results.len(), 2);
        for result in results {
            assert!(
                !result.ok && !result.detail.contains("CA file"),
                "{result:?}"
            );
        }
    }
}
