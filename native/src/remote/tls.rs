//! QUIC settings for both ends. TLS 1.3 with a certificate on each side:
//! the client accepts only the gate certificate whose fingerprint SSH told
//! it, and the gate accepts only clients listed in `authorized_clients`.

use std::path::PathBuf;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};

use super::identity::{fingerprint, Identity};
use super::{authorized, ALPN, IDLE_TIMEOUT, KEEP_ALIVE};

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn transport() -> Arc<quinn::TransportConfig> {
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(KEEP_ALIVE));
    transport.max_idle_timeout(Some(IDLE_TIMEOUT.try_into().expect("the idle timeout fits a QUIC varint")));
    Arc::new(transport)
}

fn refused() -> Error {
    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

/// The gate's certificate must have the fingerprint SSH reported.
#[derive(Debug)]
struct PinnedServer {
    fingerprint: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if fingerprint(end_entity) == self.fingerprint { Ok(ServerCertVerified::assertion()) } else { Err(refused()) }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// A client must present a certificate listed in `authorized_clients`. The
/// file is read on every handshake, so edits apply at once.
#[derive(Debug)]
struct AuthorizedClients {
    path: PathBuf,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for AuthorizedClients {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        let presented = fingerprint(end_entity);
        if authorized::read(&self.path).contains(&presented) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(refused())
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// The gate side: its own certificate, and clients from `authorized`.
pub fn server_config(identity: &Identity, authorized: PathBuf) -> Result<quinn::ServerConfig, String> {
    let provider = provider();
    let verifier = Arc::new(AuthorizedClients { path: authorized, algorithms: provider.signature_verification_algorithms });
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.cert.clone()], PrivateKeyDer::Pkcs8(identity.key.clone_key()))
        .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport());
    Ok(config)
}

/// The dashboard side: its own certificate, and only the gate whose
/// certificate has `server_fingerprint`.
pub fn client_config(identity: &Identity, server_fingerprint: &str) -> Result<quinn::ClientConfig, String> {
    let provider = provider();
    let verifier = Arc::new(PinnedServer {
        fingerprint: server_fingerprint.to_owned(),
        algorithms: provider.signature_verification_algorithms,
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![identity.cert.clone()], PrivateKeyDer::Pkcs8(identity.key.clone_key()))
        .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport());
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::authorized;
    use crate::remote::identity::{fingerprint, load_or_create};
    use std::net::SocketAddr;
    use std::time::Duration;

    /// A gate stand-in: echoes the first stream of every accepted connection.
    fn echo_server(identity: &Identity, authorized: PathBuf) -> SocketAddr {
        let config = server_config(identity, authorized).unwrap();
        let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = endpoint.local_addr().unwrap();
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                tokio::spawn(async move {
                    let Ok(connection) = incoming.await else { return };
                    if let Ok((mut send, mut recv)) = connection.accept_bi().await
                        && let Ok(data) = recv.read_to_end(64).await
                    {
                        let _ = send.write_all(&data).await;
                        let _ = send.finish();
                    }
                    connection.closed().await;
                });
            }
        });
        addr
    }

    async fn round_trip(identity: &Identity, pin: &str, addr: SocketAddr) -> Result<Vec<u8>, String> {
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).map_err(|e| e.to_string())?;
        endpoint.set_default_client_config(client_config(identity, pin)?);
        let exchange = async {
            let connection = endpoint.connect(addr, "westerm").map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
            let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
            send.write_all(b"ping").await.map_err(|e| e.to_string())?;
            send.finish().map_err(|e| e.to_string())?;
            recv.read_to_end(64).await.map_err(|e| e.to_string())
        };
        tokio::time::timeout(Duration::from_secs(5), exchange).await.map_err(|_| "timed out".to_owned())?
    }

    struct Pair {
        _dir: tempfile::TempDir,
        gate: Identity,
        client: Identity,
        authorized: PathBuf,
    }

    fn pair() -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let gate = load_or_create(dir.path(), "gate").unwrap();
        let client = load_or_create(dir.path(), "client").unwrap();
        let authorized = dir.path().join("authorized_clients");
        Pair { _dir: dir, gate, client, authorized }
    }

    #[tokio::test]
    async fn an_authorized_client_reaches_the_pinned_server() {
        let p = pair();
        authorized::add(&p.authorized, &fingerprint(&p.client.cert)).unwrap();
        let addr = echo_server(&p.gate, p.authorized.clone());
        assert_eq!(round_trip(&p.client, &fingerprint(&p.gate.cert), addr).await.unwrap(), b"ping");
    }

    #[tokio::test]
    async fn a_client_refuses_a_server_with_another_certificate() {
        let p = pair();
        authorized::add(&p.authorized, &fingerprint(&p.client.cert)).unwrap();
        let addr = echo_server(&p.gate, p.authorized.clone());
        let wrong = fingerprint(&p.client.cert);
        assert!(round_trip(&p.client, &wrong, addr).await.is_err());
    }

    #[tokio::test]
    async fn the_server_refuses_an_unknown_client() {
        let p = pair();
        let addr = echo_server(&p.gate, p.authorized.clone());
        assert!(round_trip(&p.client, &fingerprint(&p.gate.cert), addr).await.is_err());
    }

    #[tokio::test]
    async fn the_server_rereads_the_authorized_file() {
        let p = pair();
        let addr = echo_server(&p.gate, p.authorized.clone());
        let pin = fingerprint(&p.gate.cert);
        assert!(round_trip(&p.client, &pin, addr).await.is_err());
        authorized::add(&p.authorized, &fingerprint(&p.client.cert)).unwrap();
        assert_eq!(round_trip(&p.client, &pin, addr).await.unwrap(), b"ping");
    }
}
