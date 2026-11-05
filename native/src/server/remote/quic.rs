//! A QUIC connection to a server's gate. Each pane opens its own stream on
//! it, so a busy pane does not hold the others up. When this computer's
//! address changes, QUIC moves the connection to the new path.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
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
        let fail = |detail: String| Failure::new(RemoteFailure::Transport, detail);
        let addr = tokio::net::lookup_host((hostname, port))
            .await
            .map_err(|e| fail(format!("cannot resolve {hostname}: {e}")))?
            .next()
            .ok_or_else(|| fail(format!("{hostname} has no address")))?;
        let bind: SocketAddr =
            if addr.is_ipv6() { (Ipv6Addr::UNSPECIFIED, 0).into() } else { (Ipv4Addr::UNSPECIFIED, 0).into() };
        let mut endpoint = quinn::Endpoint::client(bind).map_err(|e| fail(e.to_string()))?;
        endpoint.set_default_client_config(tls::client_config(identity, fingerprint).map_err(fail)?);
        let connecting = endpoint.connect(addr, "westerm").map_err(|e| fail(e.to_string()))?;
        let connection = tokio::time::timeout(timeout, connecting)
            .await
            .map_err(|_| fail(format!("no QUIC answer from {addr} within {} s (is UDP {port} open?)", timeout.as_secs())))?
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
