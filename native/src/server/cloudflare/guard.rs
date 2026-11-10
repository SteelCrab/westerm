//! `westerm tunnel-guard -- <program> <args…>`: runs cloudflared for the
//! dashboard and ends it when the dashboard ends. The dashboard holds the
//! guard's stdin open and never writes to it. However the dashboard ends,
//! even by SIGKILL, the OS closes that pipe; the guard reads end-of-file and
//! ends cloudflared. cloudflared itself does not watch its stdin. This is the
//! idea of KEEP_OPEN in server/remote/tunnel.rs, on every OS.

use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The exit code: the child's, 0 after the guard ended it, 127 when it did not start.
pub fn run(program: OsString, args: Vec<OsString>) -> i32 {
    let mut child = match Command::new(&program).args(&args).stdin(Stdio::null()).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("westerm: {}: {error}", program.to_string_lossy());
            return 127;
        }
    };
    let closed = Arc::new(AtomicBool::new(false));
    {
        let closed = Arc::clone(&closed);
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            closed.store(true, Ordering::SeqCst);
        });
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(1),
            Ok(None) => {}
            Err(_) => return 1,
        }
        if closed.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return 0;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
