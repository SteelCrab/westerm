//! The dashboard's side of remote westerm: preparing a server over SSH and
//! reaching its host over QUIC, or an SSH tunnel when UDP is blocked.

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::RemoteFailure;

pub mod binary;
pub mod manager;
pub mod quic;
pub mod records;
pub mod ssh;
#[cfg(unix)]
pub mod tunnel;

pub use manager::RemoteManager;

/// A byte stream to a remote host: a QUIC stream or a tunnelled socket.
pub trait HostIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> HostIo for T {}

pub type HostStream = Box<dyn HostIo>;

/// Why a remote westerm could not connect, with the detail a person needs
/// (ssh's own words, a path, a version).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub reason: RemoteFailure,
    pub detail: String,
}

impl Failure {
    pub fn new(reason: RemoteFailure, detail: impl Into<String>) -> Self {
        Self { reason, detail: detail.into() }
    }
}
