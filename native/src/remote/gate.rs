//! `westerm gate`: the server's QUIC entrance to its dashboard. It accepts
//! only clients in `authorized_clients`, and joins each stream to the server
//! dashboard's Unix socket. It checks no request: the dashboard answers only
//! the routes a remote window needs, and the host stays behind it.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;

use super::daemon::{self, Daemon, Record};
use super::punch_gate::{self, Limits, Puncher};
use super::{dashboard, identity, remote_dir, tls, DEFAULT_PORT, PORT_SPAN};
use crate::ipc;

/// What a running gate wrote to `gate.json` in its state directory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GateRecord {
    pub pid: u32,
    pub port: u16,
    /// `version_line()` of the binary that runs it.
    pub version: String,
}

pub static GATE: Daemon = Daemon::new("gate");

impl Record for GateRecord {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn version(&self) -> &str {
        &self.version
    }
}

/// Asks the running gate to stop and waits up to 5 s. False when none ran.
pub async fn stop_running(state: &Path) -> io::Result<bool> {
    GATE.stop_running(state).await
}

/// WESTERM_GATE_PORT picks one port (0: any free one); otherwise the gate
/// tries 4317 and the next ports.
pub fn ports_from(env: Option<&str>) -> Vec<u16> {
    match env.and_then(|text| text.trim().parse::<u16>().ok()) {
        Some(port) => vec![port],
        None => (DEFAULT_PORT..DEFAULT_PORT + PORT_SPAN).collect(),
    }
}

/// The first port that binds, on IPv6 (dual-stack where the system allows)
/// and else IPv4. Another user's gate may hold a port.
pub fn bind_first(config: quinn::ServerConfig, ports: &[u16]) -> io::Result<(quinn::Endpoint, u16)> {
    for &port in ports {
        for addr in [SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)), SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))] {
            if let Ok(endpoint) = quinn::Endpoint::server(config.clone(), addr) {
                let bound = endpoint.local_addr()?.port();
                return Ok((endpoint, bound));
            }
        }
    }
    let range = match ports {
        [only] => only.to_string(),
        [first, .., last] => format!("{first}–{last}"),
        [] => "the list".to_owned(),
    };
    Err(io::Error::new(io::ErrorKind::AddrInUse, format!("no free UDP port in {range}")))
}

/// `westerm gate` for the state directory `state` (see state_dir()).
pub async fn run(state: &Path) -> io::Result<()> {
    let lock = GATE.lock(state)?;
    let dir = remote_dir(state)?;
    let identity = identity::load_or_create(&dir, "gate")?;
    let config = tls::server_config(&identity, dir.join("authorized_clients")).map_err(io::Error::other)?;
    let ports = ports_from(std::env::var("WESTERM_GATE_PORT").ok().as_deref());
    let (endpoint, port) = bind_first(config.clone(), &ports)?;
    // Before the record: whoever sees gate.json may ask for a punch at once.
    let control = punch_gate::socket_path(state);
    let puncher = match ipc::bind_socket(&control) {
        Ok(listener) => {
            let puncher = Puncher::new(config, state.to_path_buf(), Limits::from_env());
            tokio::spawn(puncher.clone().serve(listener));
            Some(puncher)
        }
        Err(error) => {
            eprintln!("gate: no control socket, so no hole punching: {error}");
            None
        }
    };
    let version = crate::version_line();
    GATE.write_record(state, &GateRecord { pid: std::process::id(), port, version: version.clone() })?;
    println!("gate {version} listening on UDP {port} (pid {})", std::process::id());

    let reason = tokio::select! {
        () = accept(endpoint.clone(), state.to_path_buf()) => "the endpoint closed",
        signal = daemon::termination() => signal,
    };
    println!("gate stopping ({reason})");
    // No new punch requests while the punched sockets close.
    if puncher.is_some() {
        let _ = std::fs::remove_file(&control);
    }
    endpoint.close(0u32.into(), b"gate stopping");
    // The close reaches the clients only while the endpoint runs; without it
    // they notice at their idle timeout. A client that never answers costs 1 s,
    // for the main port and the punched sockets together.
    let main = async {
        let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
    };
    let punched = async {
        if let Some(puncher) = &puncher {
            puncher.close_all().await;
        }
    };
    tokio::join!(main, punched);
    // gate.json goes before the lock: whoever waits for the lock may start
    // a new gate, which writes its own record.
    GATE.remove_record(state);
    drop(lock);
    Ok(())
}

async fn accept(endpoint: quinn::Endpoint, state: PathBuf) {
    while let Some(incoming) = endpoint.accept().await {
        // A Retry proves the client owns its address before any work is done.
        if !incoming.remote_address_validated() {
            let _ = incoming.retry();
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            let Ok(connection) = incoming.await else { return };
            serve_connection(connection, state).await;
        });
    }
}

/// Joins each stream of `connection` to the server dashboard's socket, until
/// the connection ends. The main port and the punched sockets share it.
pub(super) async fn serve_connection(connection: quinn::Connection, state: PathBuf) {
    while let Ok((send, recv)) = connection.accept_bi().await {
        let socket = dashboard::socket_path(&state);
        tokio::spawn(async move {
            let mut dashboard = match UnixStream::connect(&socket).await {
                Ok(dashboard) => dashboard,
                Err(error) => {
                    eprintln!("gate: {} refused a stream: {error}", socket.display());
                    return;
                }
            };
            let _ = tokio::io::copy_bidirectional(&mut tokio::io::join(recv, send), &mut dashboard).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_follow_the_override() {
        assert_eq!(ports_from(None), (4317..=4326).collect::<Vec<_>>());
        assert_eq!(ports_from(Some("0")), vec![0]);
        assert_eq!(ports_from(Some("5000")), vec![5000]);
        assert_eq!(ports_from(Some("x")), ports_from(None));
    }

    #[tokio::test]
    async fn binds_the_next_port_when_one_is_taken() {
        let v4 = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        let taken = v4.local_addr().unwrap().port();
        let _v6 = std::net::UdpSocket::bind(("::", taken));
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::remote::identity::load_or_create(dir.path(), "gate").unwrap();
        let config = crate::remote::tls::server_config(&identity, dir.path().join("authorized_clients")).unwrap();
        let (_endpoint, port) = bind_first(config, &[taken, 0]).unwrap();
        assert_ne!(port, taken);
        assert_ne!(port, 0);
    }
}
