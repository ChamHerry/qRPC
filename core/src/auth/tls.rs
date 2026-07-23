//! TLS 与 token 鉴权配置。

use crate::call::Metadata;
use std::fmt;
use std::sync::Arc;

/// TLS 证书/CA 配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsConfig {
    pub cert_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Vec<u8>,
    pub ca_certs_der: Vec<Vec<u8>>,
    pub server_name: String,
}

impl TlsConfig {
    pub fn new(
        cert_chain_der: Vec<Vec<u8>>,
        private_key_pkcs8_der: Vec<u8>,
        ca_certs_der: Vec<Vec<u8>>,
        server_name: impl Into<String>,
    ) -> Result<Self, AuthError> {
        if cert_chain_der.is_empty() {
            return Err(AuthError::MissingCertificate);
        }
        if private_key_pkcs8_der.is_empty() {
            return Err(AuthError::MissingPrivateKey);
        }
        if ca_certs_der.is_empty() {
            return Err(AuthError::MissingCaCertificate);
        }
        let server_name = server_name.into();
        if server_name.is_empty() {
            return Err(AuthError::MissingServerName);
        }
        Ok(Self {
            cert_chain_der,
            private_key_pkcs8_der,
            ca_certs_der,
            server_name,
        })
    }

    pub fn quinn_server_config(&self) -> Result<::quinn::ServerConfig, AuthError> {
        let cert_chain = self
            .cert_chain_der
            .iter()
            .cloned()
            .map(::quinn::rustls::pki_types::CertificateDer::from)
            .collect::<Vec<_>>();
        let private_key = ::quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(
            self.private_key_pkcs8_der.clone(),
        );
        ::quinn::ServerConfig::with_single_cert(cert_chain, private_key.into())
            .map_err(|err| AuthError::TlsConfig(err.to_string()))
    }

    pub fn quinn_client_config(&self) -> Result<::quinn::ClientConfig, AuthError> {
        let mut roots = ::quinn::rustls::RootCertStore::empty();
        for cert in &self.ca_certs_der {
            roots
                .add(::quinn::rustls::pki_types::CertificateDer::from(cert.clone()))
                .map_err(|err| AuthError::TlsConfig(err.to_string()))?;
        }
        ::quinn::ClientConfig::with_root_certificates(Arc::new(roots))
            .map_err(|err| AuthError::TlsConfig(err.to_string()))
    }
}

/// 可选 token 策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPolicy {
    required_token: Option<Vec<u8>>,
}

impl TokenPolicy {
    pub fn optional() -> Self {
        Self {
            required_token: None,
        }
    }

    pub fn required(token: impl Into<Vec<u8>>) -> Self {
        Self {
            required_token: Some(token.into()),
        }
    }

    pub fn verify_metadata(&self, meta: &Metadata) -> Result<(), AuthError> {
        let Some(required) = &self.required_token else {
            return Ok(());
        };
        let provided = meta
            .iter()
            .find(|(key, _)| key == ":auth")
            .map(|(_, value)| value.as_slice())
            .ok_or(AuthError::MissingToken)?;
        if provided == required.as_slice() {
            Ok(())
        } else {
            Err(AuthError::InvalidToken)
        }
    }
}

/// 鉴权配置错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingCertificate,
    MissingPrivateKey,
    MissingCaCertificate,
    MissingServerName,
    MissingToken,
    InvalidToken,
    TlsConfig(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCertificate => write!(f, "missing TLS certificate"),
            Self::MissingPrivateKey => write!(f, "missing TLS private key"),
            Self::MissingCaCertificate => write!(f, "missing TLS CA certificate"),
            Self::MissingServerName => write!(f, "missing TLS server name"),
            Self::MissingToken => write!(f, "missing auth token"),
            Self::InvalidToken => write!(f, "invalid auth token"),
            Self::TlsConfig(msg) => write!(f, "invalid TLS config: {msg}"),
        }
    }
}

impl std::error::Error for AuthError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{QuinnTransport, TlsConfig as TransportTlsConfig, Transport};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicU16, Ordering};

    static NEXT_PORT: AtomicU16 = AtomicU16::new(24_000);

    fn next_addr() -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            NEXT_PORT.fetch_add(1, Ordering::SeqCst),
        )
    }

    fn self_signed_tls_config() -> TlsConfig {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("cert");
        let cert_der = cert.cert.der().as_ref().to_vec();
        let key_der = cert.signing_key.serialize_der();
        TlsConfig::new(
            vec![cert_der.clone()],
            key_der,
            vec![cert_der],
            "localhost",
        )
        .expect("tls config")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auth_tls_config_builds_quinn_configs_and_connects() {
        let tls = self_signed_tls_config();
        let transport = QuinnTransport::with_configs(
            tls.quinn_client_config().expect("client tls"),
            tls.quinn_server_config().expect("server tls"),
            tls.server_name.clone(),
        );
        let addr = next_addr();
        let mut listener = transport
            .listen(&addr, &TransportTlsConfig::default())
            .await
            .expect("listen");

        let client_cfg = TransportTlsConfig::default();
        let client_fut = transport.connect(&addr, &client_cfg);
        let server_fut = listener.accept();
        let (_client, _server) =
            tokio::try_join!(client_fut, server_fut).expect("tls connect");
    }

    #[test]
    fn auth_token_policy_accepts_matching_token() {
        let policy = TokenPolicy::required(b"secret".to_vec());
        let meta = vec![(":auth".to_string(), b"secret".to_vec())];

        assert_eq!(policy.verify_metadata(&meta), Ok(()));
    }

    #[test]
    fn auth_token_policy_rejects_missing_or_wrong_token() {
        let policy = TokenPolicy::required(b"secret".to_vec());

        assert_eq!(policy.verify_metadata(&vec![]), Err(AuthError::MissingToken));
        assert_eq!(
            policy.verify_metadata(&vec![(":auth".to_string(), b"wrong".to_vec())]),
            Err(AuthError::InvalidToken)
        );
    }

    #[test]
    fn auth_optional_token_policy_allows_absent_token() {
        assert_eq!(TokenPolicy::optional().verify_metadata(&vec![]), Ok(()));
    }
}
