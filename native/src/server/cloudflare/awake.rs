//! Keeps the computer out of idle sleep while a tunnel is on, so the other
//! device does not lose it. Every way of holding ends with the dashboard,
//! even one killed by a signal, so no hold outlives westerm.

/// Held while it lives; dropping it lets the computer sleep again.
pub struct KeepAwake {
    #[cfg(unix)]
    child: std::process::Child,
    /// Dropping the sender wakes the holding thread, which lets go.
    #[cfg(windows)]
    _release: std::sync::mpsc::Sender<()>,
}

/// macOS: `caffeinate -w` ends by itself when the dashboard's pid does.
#[cfg(target_os = "macos")]
pub fn hold() -> Option<KeepAwake> {
    use std::process::{Command, Stdio};
    let child = Command::new("caffeinate")
        .args(["-i", "-w", &std::process::id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    Some(KeepAwake { child })
}

/// Linux: systemd-inhibit holds while `cat` runs; `cat` ends when the
/// dashboard's end of its stdin closes, however the dashboard ends.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn hold() -> Option<KeepAwake> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("systemd-inhibit")
        .args(["--what=idle:sleep", "--who=westerm", "--why=Cloudflare tunnel is on", "--mode=block", "cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Without a session bus or polkit's yes, systemd-inhibit exits at once.
    std::thread::sleep(std::time::Duration::from_millis(300));
    if child.try_wait().ok().flatten().is_some() {
        return None;
    }
    Some(KeepAwake { child })
}

/// Windows: a thread's execution state lasts until the thread changes it
/// or the process ends.
#[cfg(windows)]
pub fn hold() -> Option<KeepAwake> {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let (started, held) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        // SAFETY: plain Win32 calls with constant flags.
        let ok = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } != 0;
        let _ = started.send(ok);
        if ok {
            let _ = wait.recv();
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    });
    held.recv().ok().filter(|ok| *ok)?;
    Some(KeepAwake { _release: release })
}

#[cfg(unix)]
impl Drop for KeepAwake {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn dropping_releases_the_hold() {
        // A computer that cannot hold (no systemd-inhibit, as on some CI) passes.
        let Some(hold) = hold() else { return };
        let pid = hold.child.id() as libc::pid_t;
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "the hold runs");
        drop(hold);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(std::time::Instant::now() < deadline, "the hold ends");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[cfg(windows)]
    #[test]
    fn hold_and_drop_do_not_panic() {
        drop(hold());
    }
}
