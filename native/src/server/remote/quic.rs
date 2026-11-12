//! A QUIC connection to a server's gate. Each pane opens its own stream on
//! it, so a busy pane does not hold the others up. When this computer's
//! address changes, QUIC moves the connection to the new path.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use super::{Failure, HostStream};
use crate::protocol::RemoteFailure;
use crate::remote::identity::Identity;
use crate::remote::tls;

pub struct QuicLink {
    /// Its own UDP socket: a new link after a network change starts fresh.
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
}

impl QuicLink {
    /// Connects to `hostname:port`, trusting only the gate certificate with
    /// `fingerprint`.
    pub async fn connect(
        identity: &Identity,
        hostname: &str,
        port: u16,
        fingerprint: &str,
        timeout: Duration,
    ) -> Result<Self, Failure> {
        let addr = tokio::net::lookup_host((hostname, port))
            .await
            .map_err(|e| fail(format!("cannot resolve {hostname}: {e}")))?
            .next()
            .ok_or_else(|| fail(format!("{hostname} has no address")))?;
        let bind: SocketAddr =
            if addr.is_ipv6() { (Ipv6Addr::UNSPECIFIED, 0).into() } else { (Ipv4Addr::UNSPECIFIED, 0).into() };
        let endpoint = quinn::Endpoint::client(bind).map_err(|e| fail(e.to_string()))?;
        let silence = format!("no QUIC answer from {addr} within {} s (is UDP {port} open?)", timeout.as_secs());
        Self::handshake(endpoint, identity, addr, fingerprint, timeout, silence).await
    }

    /// Like `connect`, from `socket` to `addr`: the socket STUN was done on,
    /// so the NAT mapping the server punched towards is the one used.
    pub async fn connect_on(
        socket: std::net::UdpSocket,
        identity: &Identity,
        addr: SocketAddr,
        fingerprint: &str,
        timeout: Duration,
    ) -> Result<Self, Failure> {
        let endpoint = quinn::Endpoint::new(quinn::EndpointConfig::default(), None, socket, Arc::new(quinn::TokioRuntime))
            .map_err(|e| fail(e.to_string()))?;
        let silence = format!("no QUIC answer through the punched path to {addr} within {} s", timeout.as_secs());
        Self::handshake(endpoint, identity, addr, fingerprint, timeout, silence).await
    }

    /// `silence` is the error when the gate does not answer within `timeout`.
    async fn handshake(
        mut endpoint: quinn::Endpoint,
        identity: &Identity,
        addr: SocketAddr,
        fingerprint: &str,
        timeout: Duration,
        silence: String,
    ) -> Result<Self, Failure> {
        endpoint.set_default_client_config(tls::client_config(identity, fingerprint).map_err(fail)?);
        let connecting = endpoint.connect(addr, "westerm").map_err(|e| fail(e.to_string()))?;
        let connection = tokio::time::timeout(timeout, connecting)
            .await
            .map_err(|_| fail(silence))?
            .map_err(|e| fail(format!("QUIC to {addr}: {e}")))?;
        Ok(Self { _endpoint: endpoint, connection })
    }

    pub fn opener(&self) -> quinn::Connection {
        self.connection.clone()
    }

    pub fn is_alive(&self) -> bool {
        self.connection.close_reason().is_none()
    }
}

fn fail(detail: String) -> Failure {
    Failure::new(RemoteFailure::Transport, detail)
}

/// A new stream on `connection`.
pub async fn open(connection: &quinn::Connection) -> io::Result<HostStream> {
    let (send, recv) = connection.open_bi().await.map_err(io::Error::other)?;
    Ok(Box::new(tokio::io::join(recv, send)))
}

impl Drop for QuicLink {
    fn drop(&mut self) {
        self.connection.close(0u32.into(), b"closed by the dashboard");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_on_uses_the_given_socket() {
        let dir = tempfile::tempdir().unwrap();
        let gate = crate::remote::identity::load_or_create(dir.path(), "gate").unwrap();
        let client = crate::remote::identity::load_or_create(dir.path(), "client").unwrap();
        let authorized = dir.path().join("authorized_clients");
        crate::remote::authorized::add(&authorized, &crate::remote::identity::fingerprint(&client.cert)).unwrap();
        let config = crate::remote::tls::server_config(&gate, authorized).unwrap();
        let server = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let own = socket.local_addr().unwrap();
        let accept = tokio::spawn(async move { server.accept().await.unwrap().await.unwrap().remote_address() });
        let fingerprint = crate::remote::identity::fingerprint(&gate.cert);
        let link = QuicLink::connect_on(socket, &client, addr, &fingerprint, Duration::from_secs(3)).await.unwrap();
        assert_eq!(accept.await.unwrap(), own, "the server sees the socket STUN was done on");
        assert!(link.is_alive());
    }
}
