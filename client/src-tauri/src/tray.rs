//! System tray integration: tray icon with menu, unread badge, and
//! minimize-to-tray behavior.
//!
//! - Left-clicking the tray icon (or the "Show Kaiku" menu item) restores and focuses the main
//!   window.
//! - Closing the main window hides it to the tray instead of quitting; "Quit Kaiku" in the tray
//!   menu exits for real (see the window-event handler in `lib.rs`).
//! - The unread badge is driven by the frontend via the `tray_set_unread` command: tooltip
//!   everywhere, plus a text badge next to the icon on platforms that support tray titles (macOS,
//!   most Linux trays).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// Tray icon id, used to look the handle back up for badge updates.
pub const TRAY_ID: &str = "kaiku-tray";

/// Label of the primary application window (tauri.conf.json default).
const MAIN_WINDOW_LABEL: &str = "main";

/// Build the tray icon with its menu. Called once from the app setup hook.
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Kaiku", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Kaiku", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Kaiku")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Restore, un-minimize, and focus the main window.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Update the tray's unread indicator.
///
/// Tooltip is supported on all platforms; `set_title` renders a text badge
/// next to the icon on macOS and most Linux status trays, and is a no-op
/// elsewhere.
pub fn update_unread_badge<R: Runtime>(app: &AppHandle<R>, count: u64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if count > 0 {
        let _ = tray.set_tooltip(Some(format!("Kaiku — {count} unread")));
        let _ = tray.set_title(Some(count.to_string()));
    } else {
        let _ = tray.set_tooltip(Some("Kaiku"));
        let _ = tray.set_title(None::<String>);
    }
}
