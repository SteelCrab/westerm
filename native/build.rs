// The PTY host build id. The dashboard replaces a running host whose id
// differs, which restarts every shell (screens are restored), so the id
// covers only the files the host runs. Line ends are hashed as LF, so a
// Windows checkout (CRLF) gets the same id as macOS and Linux: a remote
// Linux host must match the dashboard that installed it.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};

const HOST_SOURCES: &[&str] = &[
    "src/protocol.rs",
    "src/i18n.rs",
    "src/ipc.rs",
    "src/time.rs",
    "src/spawn.rs",
    "src/quote.rs",
    "src/cwd.rs",
    "src/git.rs",
    "src/process.rs",
    "src/state_dir.rs",
    "src/screen.rs",
    "src/pixels/mod.rs",
    "src/pixels/apc.rs",
    "src/pixels/canvas.rs",
    "src/pixels/kitty.rs",
    "src/host/mod.rs",
    "src/host/store.rs",
    "src/host/session.rs",
    "src/host/registry.rs",
    "src/host/agent.rs",
    "src/plugins/mod.rs",
    "src/plugins/docker.rs",
    "src/plugins/external.rs",
];

fn main() {
    let mut hasher = DefaultHasher::new();
    for file in HOST_SOURCES {
        println!("cargo:rerun-if-changed={file}");
        file.hash(&mut hasher);
        if let Ok(bytes) = fs::read(file) {
            String::from_utf8_lossy(&bytes).replace("\r\n", "\n").as_bytes().hash(&mut hasher);
        }
    }
    println!("cargo:rustc-env=WESTERM_HOST_BUILD=rs-{:016x}", hasher.finish());
    // Which release archive an update downloads (src/update.rs).
    println!("cargo:rustc-env=WESTERM_TARGET={}", std::env::var("TARGET").unwrap_or_default());
}
