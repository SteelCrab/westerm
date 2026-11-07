fn main() {
    // `app_request` gets allow-/deny- permissions; capabilities/dashboard.json
    // grants it to the dashboard on 127.0.0.1.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["app_request"])),
    )
    .expect("tauri-build failed");
}
