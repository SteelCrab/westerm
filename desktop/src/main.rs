//! westerm-app: a desktop window around the westerm dashboard, for macOS,
//! Windows, and Linux.
//!
//! - Uses a westerm that already serves on 127.0.0.1:$PORT (default 4317),
//!   for example one started in a terminal, so both share the same sessions.
//! - Otherwise starts the westerm installed next to the app on that port, or
//!   on any free port when another program holds it (state.rs, server.rs).
//! - Quitting stops only a server the app started. The PTY host keeps
//!   running in its own session, so shells survive as they do with the CLI.
//! - The page talks to the app through `window.westermApp` (bridge.rs).

// No console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
#[cfg(target_os = "macos")]
mod menu;
mod server;
mod state;
#[cfg(target_os = "macos")]
mod strings;
mod update;
mod windows;

use std::sync::Arc;

use tauri::{Manager, RunEvent};
use tauri_plugin_window_state::StateFlags;

use crate::server::Log;
use crate::state::Shared;

fn main() {
    let builder = tauri::Builder::default();
    // A second launch brings the open window forward instead of taking the
    // sessions away from it. macOS keeps one copy of an app by itself, and
    // an update opens the new copy while this one quits.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| windows::show_main(app)));
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle);

    let app = builder
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![bridge::app_request])
        .setup(|app| {
            let log = app.path().app_log_dir().unwrap_or_else(|_| std::env::temp_dir()).join("westerm.log");
            app.manage(Arc::new(Shared::new(Log::open(log))));
            update::clean_up();
            windows::create_main(app.handle())?;
            state::connect(app.handle().clone());
            Ok(())
        })
        .on_window_event(windows::on_event)
        .build(tauri::generate_context!())
        .expect("westerm-app could not start");

    app.run(|app, event| match event {
        // The dashboard server only. The PTY host and its shells live on.
        RunEvent::Exit => state::shared(app).stop_server(),
        // A click on the Dock icon brings the main window back when it is
        // closed or minimized, even while remote windows are open.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => windows::show_main(app),
        _ => {}
    });
}
