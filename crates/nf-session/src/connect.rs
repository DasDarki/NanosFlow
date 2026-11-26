use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use nf_relay::fingerprint_of;
use nf_relay::protocol::{ALPN, DEFAULT_PORT};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, Endpoint};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::SessionError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayAddress {
    pub host: String,
    pub port: u16,
    pub fingerprint: Option<String>,
}

impl FromStr for RelayAddress {
    type Err = SessionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || SessionError::InvalidRelay(value.to_owned());
        let (address, fingerprint) = match value.trim().split_once('#') {
            Some((address, fingerprint)) => (address, Some(fingerprint.trim().to_ascii_lowercase())),
            None => (value.trim(), None),
        };
        if fingerprint.as_ref().is_some_and(|fingerprint| fingerprint.len() != 64 || !fingerprint.chars().all(|character| character.is_ascii_hexdigit())) {
            return Err(invalid());
        }
        let (host, port) = if let Some(bracketed) = address.strip_prefix('[') {
            let (host, rest) = bracketed.split_once(']').ok_or_else(invalid)?;
            let port = rest.strip_prefix(':').map(str::parse).transpose().map_err(|_| invalid())?;
            (host, port)
        } else {
            match address.rsplit_once(':') {
                Some((host, port)) if !host.contains(':') => (host, Some(port.parse().map_err(|_| invalid())?)),
                _ => (address, None),
            }
        };
        if host.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            host: host.to_owned(),
            port: port.unwrap_or(DEFAULT_PORT),
            fingerprint,
        })
    }
}

impl fmt::Display for RelayAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(formatter, "[{}]:{}", self.host, self.port)?;
        } else {
            write!(formatter, "{}:{}", self.host, self.port)?;
        }
        if let Some(fingerprint) = &self.fingerprint {
            write!(formatter, "#{fingerprint}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct PinnedCertificate {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint_of(end_entity) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("the relay certificate does not match the fingerprint".to_owned()))
        }
    }

    fn verify_tls12_signature(&self, message: &[u8], certificate: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, certificate, signature, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], certificate: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, certificate, signature, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn client_config(relay: &RelayAddress) -> Result<quinn::ClientConfig, SessionError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| SessionError::Connect(error.to_string()))?;
    let mut tls = match &relay.fingerprint {
        Some(fingerprint) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCertificate {
                fingerprint: fingerprint.clone(),
                provider,
            }))
            .with_no_client_auth(),
        None => {
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(|error| SessionError::Connect(error.to_string()))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(nf_relay::transport_config()));
    Ok(config)
}

pub(crate) async fn connect(relay: &RelayAddress) -> Result<(Endpoint, Connection), SessionError> {
    let address: SocketAddr = tokio::net::lookup_host((relay.host.as_str(), relay.port))
        .await
        .map_err(|error| SessionError::Connect(format!("{}: {error}", relay.host)))?
        .next()
        .ok_or_else(|| SessionError::Connect(format!("{} could not be resolved", relay.host)))?;
    let local: SocketAddr = if address.is_ipv4() { (Ipv4Addr::UNSPECIFIED, 0).into() } else { (Ipv6Addr::UNSPECIFIED, 0).into() };
    let mut endpoint = Endpoint::client(local).map_err(|error| SessionError::Connect(error.to_string()))?;
    endpoint.set_default_client_config(client_config(relay)?);
    let connection = endpoint
        .connect(address, &relay.host)
        .map_err(|error| SessionError::Connect(error.to_string()))?
        .await
        .map_err(|error| SessionError::Connect(error.to_string()))?;
    Ok((endpoint, connection))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relay_addresses() {
        let fingerprint = "ab".repeat(32);
        let parsed: RelayAddress = format!("relay.example.com:9000#{}", fingerprint.to_uppercase()).parse().unwrap();
        assert_eq!(
            parsed,
            RelayAddress {
                host: "relay.example.com".into(),
                port: 9000,
                fingerprint: Some(fingerprint.clone()),
            }
        );
        assert_eq!(parsed.to_string().parse::<RelayAddress>().unwrap(), parsed);
        assert_eq!("relay.example.com".parse::<RelayAddress>().unwrap().port, DEFAULT_PORT);
        let ipv6: RelayAddress = "[::1]:7901".parse().unwrap();
        assert_eq!((ipv6.host.as_str(), ipv6.port), ("::1", 7901));
        assert!("host:port".parse::<RelayAddress>().is_err());
        assert!("host#short".parse::<RelayAddress>().is_err());
    }
}
