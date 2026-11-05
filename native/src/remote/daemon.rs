//! What the server's background commands (`gate`, `remote-dashboard`) share:
//! a lock that keeps one copy running, a JSON record of that copy, and a way
//! to stop it.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::state_dir::private_file;

/// What every daemon's record tells: which process runs, and which build.
pub trait Record: DeserializeOwned {
    fn pid(&self) -> u32;
    /// `version_line()` of the binary that runs it.
    fn version(&self) -> &str;
}

/// One background command of a state directory. Its files are
/// `<name>.lock` and `<name>.json`.
pub struct Daemon {
    name: &'static str,
}

impl Daemon {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub fn record_path(&self, state: &Path) -> PathBuf {
        state.join(format!("{}.json", self.name))
    }

    pub fn log_path(&self, state: &Path) -> PathBuf {
        state.join(format!("{}.log", self.name))
    }

    fn lock_path(&self, state: &Path) -> PathBuf {
        state.join(format!("{}.lock", self.name))
    }

    /// Takes the lock a running copy holds for as long as it runs. Drop the
    /// file to release it. `AddrInUse` when another copy holds it.
    pub fn lock(&self, state: &Path) -> io::Result<File> {
        let lock = private_file().create(true).truncate(false).write(true).open(self.lock_path(state))?;
        if lock.try_lock().is_err() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("another {} is already running", self.name)));
        }
        Ok(lock)
    }

    pub fn is_running(&self, state: &Path) -> bool {
        let Ok(file) = private_file().create(true).truncate(false).write(true).open(self.lock_path(state)) else {
            return false;
        };
        file.try_lock().is_err()
    }

    pub fn read_record<T: DeserializeOwned>(&self, state: &Path) -> Option<T> {
        serde_json::from_slice(&fs::read(self.record_path(state)).ok()?).ok()
    }

    /// Written under a temporary name and renamed, so a reader never sees half of it.
    pub fn write_record<T: Serialize>(&self, state: &Path, record: &T) -> io::Result<()> {
        let temp = state.join(format!("{}.json.{}", self.name, std::process::id()));
        let mut file = private_file().create(true).truncate(true).write(true).open(&temp)?;
        file.write_all(serde_json::to_string(record).map_err(io::Error::other)?.as_bytes())?;
        fs::rename(&temp, self.record_path(state))
    }

    pub fn remove_record(&self, state: &Path) {
        let _ = fs::remove_file(self.record_path(state));
    }

    /// Asks the running copy to stop and waits up to 5 s. False when none ran.
    pub async fn stop_running(&self, state: &Path) -> io::Result<bool> {
        #[derive(Deserialize)]
        struct Pid {
            pid: u32,
        }

        if !self.is_running(state) {
            self.remove_record(state);
            return Ok(false);
        }
        let name = self.name;
        let Pid { pid } = self
            .read_record(state)
            .ok_or_else(|| io::Error::other(format!("a {name} holds {name}.lock but {name}.json is missing")))?;
        // SAFETY: kill only sends a signal to the process the record names.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        for _ in 0..50 {
            if !self.is_running(state) {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(io::Error::other(format!("the remote {name} did not stop within 5 s")))
    }
}

/// Resolves with the name of the first stop signal that arrives.
pub async fn termination() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) =
        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
        _ = hangup.recv() => "SIGHUP",
    }
}
