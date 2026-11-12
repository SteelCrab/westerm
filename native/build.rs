// The PTY host build id. The dashboard replaces a running host whose id
// differs, which restarts every shell (screens are restored), so the id
// covers only the files the host runs. Line ends are hashed as LF, so a
// Windows checkout (CRLF) gets the same id as macOS and Linux: a remote
// Linux host must match the dashboard that installed it.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

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
    "src/plugins/db.rs",
    "src/plugins/external.rs",
];

fn main() {
    // Windows (MSVC): the Visual C++ runtime goes into the executable. A
    // clean Windows has no VCRUNTIME140.dll, so westerm.exe would stop with a
    // "DLL was not found" dialog. The Universal CRT stays a DLL: it is part
    // of Windows. It does nothing for other targets. The desktop app's
    // executable gets the same from tauri-build.
    static_vcruntime::metabuild();

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
    println!("cargo:rustc-env=WESTERM_WEB_DIST={}", web_dist().display());
}

fn web_dist() -> PathBuf {
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let candidates = [manifest.join("../dist"), manifest.join("web-dist")];
    // Track both locations so creating or removing the source build switches the path.
    for directory in &candidates {
        println!("cargo:rerun-if-changed={}", directory.display());
    }
    for directory in candidates {
        if directory.join("index.html").is_file() {
            return fs::canonicalize(&directory).unwrap_or_else(|error| {
                panic!(
                    "cannot resolve web assets at {}: {error}",
                    directory.display()
                )
            });
        }
    }
    panic!(
        "westerm web assets are missing: run npm ci and npx vite build in the repository root; published crates must include web-dist/index.html"
    );
}
