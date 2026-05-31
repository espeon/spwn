use anyhow::Context;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};

/// mTLS config loaded from env vars. None = plaintext gRPC (default).
///
/// Set all three to enable:
///   GRPC_TLS_CA_PATH   — PEM CA cert (used to verify the peer)
///   GRPC_TLS_CERT_PATH — PEM cert for this service
///   GRPC_TLS_KEY_PATH  — PEM private key for this service
#[derive(Clone)]
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

/// Open a gRPC channel to an agent, with mTLS if configured.
pub async fn agent_channel(addr: &str, tls: Option<&GrpcTls>) -> anyhow::Result<Channel> {
    let ep = Channel::from_shared(addr.to_string())?;
    let ch = match tls {
        Some(t) => ep.tls_config(t.client_config())?.connect().await?,
        None => ep.connect().await?,
    };
    Ok(ch)
}
