// The PTY host build id. The dashboard replaces a running host whose id
// differs, which restarts every shell (screens are restored), so the id
// covers only the files the host runs.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};

const HOST_SOURCES: &[&str] = &[
    "src/protocol.rs",
    "src/spawn.rs",
    "src/cwd.rs",
    "src/state_dir.rs",
    "src/screen.rs",
    "src/host/mod.rs",
    "src/host/store.rs",
    "src/host/session.rs",
    "src/host/registry.rs",
];

fn main() {
    let mut hasher = DefaultHasher::new();
    for file in HOST_SOURCES {
        println!("cargo:rerun-if-changed={file}");
        file.hash(&mut hasher);
        if let Ok(bytes) = fs::read(file) {
            bytes.hash(&mut hasher);
        }
    }
    println!("cargo:rustc-env=WESTERM_HOST_BUILD=rs-{:016x}", hasher.finish());
}
