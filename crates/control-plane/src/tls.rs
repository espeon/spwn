use anyhow::Context;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};

/// mTLS config loaded from env vars. None = plaintext gRPC (default).
///
/// Set all three to enable:
///   GRPC_TLS_CA_PATH   — PEM CA cert (used to verify the peer)
///   GRPC_TLS_CERT_PATH — PEM cert for this service
///   GRPC_TLS_KEY_PATH  — PEM private key for this service
#[derive(Clone, Debug)]
pub struct GrpcTls {
    ca: Certificate,
    identity: Identity,
}

impl GrpcTls {
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let ca_path = std::env::var("GRPC_TLS_CA_PATH").ok();
        let cert_path = std::env::var("GRPC_TLS_CERT_PATH").ok();
        let key_path = std::env::var("GRPC_TLS_KEY_PATH").ok();

        match (ca_path, cert_path, key_path) {
            (None, None, None) => Ok(None),
            (Some(ca), Some(cert), Some(key)) => {
                let ca_pem = std::fs::read(&ca)
                    .with_context(|| format!("read GRPC_TLS_CA_PATH={ca}"))?;
                let cert_pem = std::fs::read(&cert)
                    .with_context(|| format!("read GRPC_TLS_CERT_PATH={cert}"))?;
                let key_pem = std::fs::read(&key)
                    .with_context(|| format!("read GRPC_TLS_KEY_PATH={key}"))?;
                Ok(Some(Self {
                    ca: Certificate::from_pem(ca_pem),
                    identity: Identity::from_pem(cert_pem, key_pem),
                }))
            }
            _ => anyhow::bail!(
                "GRPC_TLS_CA_PATH, GRPC_TLS_CERT_PATH, and GRPC_TLS_KEY_PATH \
                 must all be set or all be unset"
            ),
        }
    }

    pub fn server_config(&self) -> ServerTlsConfig {
        ServerTlsConfig::new()
            .identity(self.identity.clone())
            .client_ca_root(self.ca.clone())
    }

    pub fn client_config(&self) -> ClientTlsConfig {
        ClientTlsConfig::new()
            .ca_certificate(self.ca.clone())
            .identity(self.identity.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::GrpcTls;
    use std::io::Write;

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        // Serial env manipulation — not safe to run in parallel with other
        // tests that touch the same vars. Mark callers with #[serial] if needed.
        let old: Vec<(&str, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => unsafe { std::env::set_var(k, val) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
        f();
        for (k, v) in old {
            match v {
                Some(val) => unsafe { std::env::set_var(k, val) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }

    fn write_pem(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    // Minimal self-signed CA cert PEM for testing (not cryptographically valid,
    // just structurally valid enough to exercise the loading path).
    const DUMMY_PEM: &str = "-----BEGIN CERTIFICATE-----\nZA==\n-----END CERTIFICATE-----\n";
    const DUMMY_KEY: &str = "-----BEGIN PRIVATE KEY-----\nZA==\n-----END PRIVATE KEY-----\n";

    #[test]
    fn no_env_vars_returns_none() {
        with_env(
            &[
                ("GRPC_TLS_CA_PATH", None),
                ("GRPC_TLS_CERT_PATH", None),
                ("GRPC_TLS_KEY_PATH", None),
            ],
            || {
                let result = GrpcTls::from_env();
                assert!(result.is_ok());
                assert!(result.unwrap().is_none());
            },
        );
    }

    #[test]
    fn partial_env_vars_returns_error() {
        with_env(
            &[
                ("GRPC_TLS_CA_PATH", Some("/some/ca.pem")),
                ("GRPC_TLS_CERT_PATH", None),
                ("GRPC_TLS_KEY_PATH", None),
            ],
            || {
                let result = GrpcTls::from_env();
                assert!(result.is_err());
                let msg = result.unwrap_err().to_string();
                assert!(msg.contains("all be set or all be unset"), "got: {msg}");
            },
        );
    }

    #[test]
    fn missing_file_returns_error() {
        with_env(
            &[
                ("GRPC_TLS_CA_PATH", Some("/nonexistent/ca.pem")),
                ("GRPC_TLS_CERT_PATH", Some("/nonexistent/cert.pem")),
                ("GRPC_TLS_KEY_PATH", Some("/nonexistent/key.pem")),
            ],
            || {
                let result = GrpcTls::from_env();
                assert!(result.is_err());
            },
        );
    }

    #[test]
    fn all_vars_set_with_valid_files_returns_some() {
        let ca = write_pem(DUMMY_PEM);
        let cert = write_pem(DUMMY_PEM);
        let key = write_pem(DUMMY_KEY);

        with_env(
            &[
                ("GRPC_TLS_CA_PATH", Some(ca.path().to_str().unwrap())),
                ("GRPC_TLS_CERT_PATH", Some(cert.path().to_str().unwrap())),
                ("GRPC_TLS_KEY_PATH", Some(key.path().to_str().unwrap())),
            ],
            || {
                let result = GrpcTls::from_env();
                assert!(result.is_ok());
                assert!(result.unwrap().is_some());
            },
        );
    }
}

/// Open a gRPC channel to an agent, with mTLS if configured.
pub async fn agent_channel(addr: &str, tls: Option<&GrpcTls>) -> anyhow::Result<Channel> {
    let ep = Channel::from_shared(addr.to_string())?;
    let ch = match tls {
        Some(t) => ep.tls_config(t.client_config())?.connect().await?,
        None => ep.connect().await?,
    };
    Ok(ch)
}
