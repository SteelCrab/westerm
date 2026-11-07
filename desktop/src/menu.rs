//! The macOS menu bar. Tab and pane switching: the page handles these keys
//! itself (src/shortcuts.ts); the menu items show them and work by click.
//! Windows and Linux have no menu bar: the page's Ctrl+Shift keys do this.

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Wry};

use crate::strings::{Language, MenuText};
use crate::windows;

const CHECK_FOR_UPDATES: &str = "check-for-updates";
const SETTINGS: &str = "settings";
const RELOAD: &str = "reload";
const OPEN_IN_BROWSER: &str = "open-in-browser";
/// Items whose id is "switch:<SwitchCommand JSON>".
const SWITCH: &str = "switch:";

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let t = MenuText::new(Language::system());
    let item = |id: &str, text: &str, keys: Option<&str>| MenuItem::with_id(app, id, text, true, keys);
    let switch = |text: &str, command: &str, keys: &str| item(&format!("{SWITCH}{command}"), text, Some(keys));
    let separator = || PredefinedMenuItem::separator(app);

    let info = app.package_info();
    let about = AboutMetadata {
        name: Some("westerm".into()),
        version: Some(info.version.to_string()),
        copyright: Some("Copyright © 2026 SteelCrab. MIT License.".into()),
        ..Default::default()
    };
    let westerm = Submenu::with_items(
        app,
        "westerm",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(t.about), Some(about))?,
            &item(CHECK_FOR_UPDATES, t.check_for_updates, None)?,
            &separator()?,
            &item(SETTINGS, t.settings, Some("Cmd+,"))?,
            &separator()?,
            &PredefinedMenuItem::hide(app, Some(t.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(t.hide_others))?,
            &PredefinedMenuItem::show_all(app, Some(t.show_all))?,
            &separator()?,
            &PredefinedMenuItem::quit(app, Some(t.quit))?,
        ],
    )?;

    // Copy and paste reach the terminal through the web view's own handlers.
    let edit = Submenu::with_items(
        app,
        t.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(t.undo))?,
            &PredefinedMenuItem::redo(app, Some(t.redo))?,
            &separator()?,
            &PredefinedMenuItem::cut(app, Some(t.cut))?,
            &PredefinedMenuItem::copy(app, Some(t.copy))?,
            &PredefinedMenuItem::paste(app, Some(t.paste))?,
            &PredefinedMenuItem::select_all(app, Some(t.select_all))?,
        ],
    )?;

    let view = Submenu::with_items(
        app,
        t.view,
        true,
        &[
            &item(RELOAD, t.reload, Some("Cmd+R"))?,
            &item(OPEN_IN_BROWSER, t.open_in_browser, None)?,
            &separator()?,
            &PredefinedMenuItem::fullscreen(app, Some(t.full_screen))?,
        ],
    )?;

    let tab_items = (1..=9u8)
        .map(|number| {
            let title = if number == 9 { t.last_tab.to_owned() } else { t.tab_number(number) };
            switch(&title, &format!(r#"{{"type":"tab","number":{number}}}"#), &format!("Cmd+{number}"))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let tab_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = tab_items.iter().map(|item| item as _).collect();
    let tabs = Submenu::with_items(app, t.go_to_tab, true, &tab_refs)?;

    let window = Submenu::with_items(
        app,
        t.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(t.minimize))?,
            &PredefinedMenuItem::maximize(app, Some(t.zoom))?,
            &PredefinedMenuItem::close_window(app, Some(t.close))?,
            &separator()?,
            &switch(t.next_tab, r#"{"type":"tabStep","delta":1}"#, "Cmd+Shift+]")?,
            &switch(t.previous_tab, r#"{"type":"tabStep","delta":-1}"#, "Cmd+Shift+[")?,
            &switch(t.next_pane, r#"{"type":"paneStep","delta":1}"#, "Cmd+]")?,
            &switch(t.previous_pane, r#"{"type":"paneStep","delta":-1}"#, "Cmd+[")?,
            &tabs,
        ],
    )?;
    window.set_as_windows_menu_for_nsapp()?;

    Menu::with_items(app, &[&westerm, &edit, &view, &window])
}

/// The script that raises a page event, for a menu item's id.
fn page_event(id: &str) -> Option<String> {
    if let Some(command) = id.strip_prefix(SWITCH) {
        return Some(format!("window.dispatchEvent(new CustomEvent('westerm:command', {{ detail: {command} }}))"));
    }
    let name = match id {
        SETTINGS => "westerm:settings",
        CHECK_FOR_UPDATES => "westerm:update",
        _ => return None,
    };
    Some(format!("window.dispatchEvent(new Event('{name}'))"))
}

pub fn handle(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    // Updates belong to the main window; the others go to the window in front.
    let target = if id == CHECK_FOR_UPDATES {
        windows::show_main(app);
        tauri::Manager::get_webview_window(app, windows::MAIN)
    } else {
        windows::active(app)
    };
    let Some(window) = target else { return };
    match id {
        RELOAD => windows::reload(app, &window),
        OPEN_IN_BROWSER => windows::open_in_browser(app, &window),
        _ => {
            if let Some(script) = page_event(id) {
                let _ = window.eval(script);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_items_raise_the_page_events() {
        assert_eq!(
            page_event(r#"switch:{"type":"tab","number":3}"#).unwrap(),
            r#"window.dispatchEvent(new CustomEvent('westerm:command', { detail: {"type":"tab","number":3} }))"#
        );
        assert_eq!(page_event(SETTINGS).unwrap(), "window.dispatchEvent(new Event('westerm:settings'))");
        assert_eq!(page_event(CHECK_FOR_UPDATES).unwrap(), "window.dispatchEvent(new Event('westerm:update'))");
        assert_eq!(page_event(RELOAD), None);
    }
}
