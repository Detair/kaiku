//! System tray integration: tray icon with menu, unread badge, voice controls and
//! close-to-tray behavior.
//!
//! - Left-clicking the tray icon (or the "Show Kaiku" menu item) restores and focuses the main
//!   window.
//! - Closing the main window goes through [`handle_main_close`]: with a reachable tray it hides the
//!   window (and tells the user Kaiku is still in a call, if it is); without one it only minimizes,
//!   so the window can always be found again. "Quit Kaiku" in the tray menu exits.
//! - While in voice the tray menu also offers "Leave voice" and "Mute"/"Unmute". The webview
//!   reports the voice state via `set_tray_voice_state`; the menu items are sent back to the
//!   webview as [`VOICE_ACTION_EVENT`] so the voice store stays the single owner of the call.
//! - The unread badge is driven by the frontend via the `tray_set_unread` command: tooltip
//!   everywhere, plus a text badge next to the icon on platforms that support tray titles (macOS,
//!   most Linux trays).
//!
//! User-facing text lives in [`strings`]; Rust has no i18n layer yet, so a later
//! localisation step only has to replace that module.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{IsMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Window};
use tauri_plugin_notification::NotificationExt;

/// User-facing tray and notification text (English only until Rust gets i18n).
pub mod strings {
    pub const APP_NAME: &str = "Kaiku";
    pub const MENU_SHOW: &str = "Show Kaiku";
    pub const MENU_QUIT: &str = "Quit Kaiku";
    pub const MENU_LEAVE_VOICE: &str = "Leave voice";
    pub const MENU_MUTE: &str = "Mute";
    pub const MENU_UNMUTE: &str = "Unmute";
    pub const NOTICE_IN_VOICE: &str =
        "Kaiku is still in the voice channel. Use the tray icon to leave.";
    pub const NOTICE_FIRST_CLOSE: &str =
        "Kaiku is still running. Use the tray icon to open it or quit.";
    /// Tooltip with unread messages.
    pub fn tooltip_unread(count: u64) -> String {
        format!("Kaiku — {count} unread")
    }
}

/// Tray icon id, used to look the handle back up for badge and menu updates.
pub const TRAY_ID: &str = "kaiku-tray";

/// Event emitted to the webview when a voice item of the tray menu is clicked.
/// Payload: [`VOICE_ACTION_LEAVE`] or [`VOICE_ACTION_TOGGLE_MUTE`].
pub const VOICE_ACTION_EVENT: &str = "tray-voice-action";
pub const VOICE_ACTION_LEAVE: &str = "leave";
pub const VOICE_ACTION_TOGGLE_MUTE: &str = "toggle_mute";

/// Label of the primary application window (tauri.conf.json default).
const MAIN_WINDOW_LABEL: &str = "main";

const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";
const ID_LEAVE_VOICE: &str = "leave_voice";
const ID_TOGGLE_MUTE: &str = "toggle_mute";

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window; the tray icon brings it back.
    HideToTray {
        /// The user is in a call: say so, so the mic is never live unnoticed.
        notify_voice: bool,
        /// First close this session: explain that Kaiku keeps running in the tray.
        first_time: bool,
    },
    /// No tray to come back from: minimize instead of hiding.
    Minimize,
}

/// Decide what closing the main window does. Pure; see [`handle_main_close`].
pub const fn close_action(tray_available: bool, in_voice: bool, notice_shown: bool) -> CloseAction {
    if !tray_available {
        return CloseAction::Minimize;
    }
    CloseAction::HideToTray {
        notify_voice: in_voice,
        first_time: !notice_shown,
    }
}

/// The one notification shown for a hide-to-tray close, if any. The voice notice wins over
/// the first-close notice so the user gets a single message.
const fn close_notice(notify_voice: bool, first_time: bool) -> Option<&'static str> {
    if notify_voice {
        Some(strings::NOTICE_IN_VOICE)
    } else if first_time {
        Some(strings::NOTICE_FIRST_CLOSE)
    } else {
        None
    }
}

/// Tray menu as `(id, label)` pairs for the given voice state.
fn menu_entries(in_voice: bool, muted: bool) -> Vec<(&'static str, &'static str)> {
    let mut entries = vec![(ID_SHOW, strings::MENU_SHOW)];
    if in_voice {
        entries.push((ID_LEAVE_VOICE, strings::MENU_LEAVE_VOICE));
        let mute_label = if muted {
            strings::MENU_UNMUTE
        } else {
            strings::MENU_MUTE
        };
        entries.push((ID_TOGGLE_MUTE, mute_label));
    }
    entries.push((ID_QUIT, strings::MENU_QUIT));
    entries
}

/// Tray and voice flags shared by the close handler and the tray commands.
#[derive(Debug, Default)]
pub struct TrayState {
    /// The tray icon was built successfully.
    built: AtomicBool,
    /// Last voice state reported by the webview (connecting counts as in voice).
    in_voice: AtomicBool,
    muted: AtomicBool,
    /// A close notice was already shown this session.
    notice_shown: AtomicBool,
}

/// Build the tray icon with its menu. Called once from the app setup hook, after
/// [`TrayState`] is managed.
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app, false, false)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(strings::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_SHOW => show_main_window(app),
            ID_QUIT => app.exit(0),
            ID_LEAVE_VOICE => emit_voice_action(app, VOICE_ACTION_LEAVE),
            ID_TOGGLE_MUTE => emit_voice_action(app, VOICE_ACTION_TOGGLE_MUTE),
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
    if let Some(state) = app.try_state::<TrayState>() {
        state.built.store(true, Ordering::SeqCst);
    }
    if !host_present() {
        tracing::warn!(
            "No system tray host found (e.g. GNOME without an AppIndicator extension); \
             closing the window will minimize it instead of hiding it"
        );
    }
    Ok(())
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    in_voice: bool,
    muted: bool,
) -> tauri::Result<Menu<R>> {
    let items = menu_entries(in_voice, muted)
        .into_iter()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
    Menu::with_items(app, &refs)
}

fn emit_voice_action<R: Runtime>(app: &AppHandle<R>, action: &str) {
    if let Err(err) = app.emit(VOICE_ACTION_EVENT, action) {
        tracing::error!(error = %err, action, "Failed to forward tray voice action");
    }
}

/// Restore, un-minimize, and focus the main window.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Store the webview's voice state and rebuild the tray menu when it changed.
pub fn set_voice_state<R: Runtime>(app: &AppHandle<R>, in_voice: bool, muted: bool) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let was_in_voice = state.in_voice.swap(in_voice, Ordering::SeqCst);
    let was_muted = state.muted.swap(muted, Ordering::SeqCst);
    if was_in_voice == in_voice && was_muted == muted {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app, in_voice, muted) {
        Ok(menu) => {
            if let Err(err) = tray.set_menu(Some(menu)) {
                tracing::error!(error = %err, "Failed to update tray menu");
            }
        }
        Err(err) => tracing::error!(error = %err, "Failed to build tray menu"),
    }
}

/// Close handler for the main window (its close was already prevented).
pub fn handle_main_close<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let Some(state) = app.try_state::<TrayState>() else {
        let _ = window.minimize();
        return;
    };
    let in_voice = state.in_voice.load(Ordering::SeqCst) || native_voice_active(app);
    let tray_available = state.built.load(Ordering::SeqCst) && host_present();
    let notice_shown = state.notice_shown.load(Ordering::SeqCst);

    match close_action(tray_available, in_voice, notice_shown) {
        CloseAction::HideToTray {
            notify_voice,
            first_time,
        } => {
            let _ = window.hide();
            if let Some(body) = close_notice(notify_voice, first_time) {
                state.notice_shown.store(true, Ordering::SeqCst);
                if let Err(err) = app
                    .notification()
                    .builder()
                    .title(strings::APP_NAME)
                    .body(body)
                    .show()
                {
                    tracing::warn!(error = %err, "Failed to show close-to-tray notification");
                }
            }
        }
        CloseAction::Minimize => {
            let _ = window.minimize();
        }
    }
}

/// Whether the native voice engine holds a channel. Complements the webview's report so a
/// close right after joining (before the report arrives) still counts as in voice. A
/// write-locked voice state means a join or leave is in progress: treated as in voice.
fn native_voice_active<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return false;
    };
    let result = match state.voice.try_read() {
        Ok(voice) => voice.as_ref().is_some_and(|v| v.channel_id.is_some()),
        Err(_) => true,
    };
    result
}

/// Whether a tray host can show the icon.
///
/// Tauri reports no failure here: on Linux, `TrayIconBuilder::build` succeeds even when no
/// `StatusNotifier` host is running (libappindicator then registers with nobody, so the icon
/// is invisible — GNOME without an `AppIndicator` extension). So on Linux we ask the session
/// bus for an `org.kde.StatusNotifierWatcher` with a registered host. Windows and macOS
/// always have a notification area / menu bar.
#[cfg(target_os = "linux")]
fn host_present() -> bool {
    use std::time::Duration;

    use zbus::blocking::fdo::DBusProxy;
    use zbus::blocking::{connection, Proxy};

    const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    let Ok(conn) = connection::Builder::session()
        .map(|b| b.method_timeout(Duration::from_millis(500)))
        .and_then(|b| b.build())
    else {
        return false;
    };
    let owned = || -> zbus::Result<bool> {
        Ok(DBusProxy::new(&conn)?.name_has_owner(WATCHER.try_into()?)?)
    };
    if !owned().unwrap_or(false) {
        return false;
    }
    // A watcher without a host (rare) shows nothing; a watcher that does not expose the
    // property is trusted.
    Proxy::new(&conn, WATCHER, "/StatusNotifierWatcher", WATCHER)
        .and_then(|p| p.get_property::<bool>("IsStatusNotifierHostRegistered"))
        .unwrap_or(true)
}

#[cfg(not(target_os = "linux"))]
fn host_present() -> bool {
    true
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
        let _ = tray.set_tooltip(Some(strings::tooltip_unread(count)));
        let _ = tray.set_title(Some(count.to_string()));
    } else {
        let _ = tray.set_tooltip(Some(strings::APP_NAME));
        let _ = tray.set_title(None::<String>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_without_tray_minimizes() {
        // No tray host: hiding would leave an unreachable window, in voice or not.
        for in_voice in [false, true] {
            for notice_shown in [false, true] {
                assert_eq!(
                    close_action(false, in_voice, notice_shown),
                    CloseAction::Minimize
                );
            }
        }
    }

    #[test]
    fn close_in_voice_hides_with_voice_notice() {
        // Every close during a call tells the user the mic is still live,
        // even after the first-close notice was already shown.
        for notice_shown in [false, true] {
            match close_action(true, true, notice_shown) {
                CloseAction::HideToTray { notify_voice, .. } => assert!(notify_voice),
                CloseAction::Minimize => panic!("tray available: must hide, not minimize"),
            }
        }
        assert_eq!(close_notice(true, true), Some(strings::NOTICE_IN_VOICE));
        assert_eq!(close_notice(true, false), Some(strings::NOTICE_IN_VOICE));
    }

    #[test]
    fn first_close_shows_notice_once() {
        assert_eq!(
            close_action(true, false, false),
            CloseAction::HideToTray {
                notify_voice: false,
                first_time: true
            }
        );
        assert_eq!(
            close_action(true, false, true),
            CloseAction::HideToTray {
                notify_voice: false,
                first_time: false
            }
        );
        assert_eq!(close_notice(false, true), Some(strings::NOTICE_FIRST_CLOSE));
        assert_eq!(close_notice(false, false), None);
    }

    #[test]
    fn tray_menu_offers_leave_and_mute_only_in_voice() {
        let ids = |in_voice, muted| -> Vec<&'static str> {
            menu_entries(in_voice, muted).iter().map(|e| e.0).collect()
        };
        assert_eq!(ids(false, false), vec![ID_SHOW, ID_QUIT]);
        assert_eq!(
            ids(true, false),
            vec![ID_SHOW, ID_LEAVE_VOICE, ID_TOGGLE_MUTE, ID_QUIT]
        );

        let label = |in_voice, muted, id| {
            menu_entries(in_voice, muted)
                .into_iter()
                .find(|e| e.0 == id)
                .map(|e| e.1)
        };
        assert_eq!(label(true, false, ID_TOGGLE_MUTE), Some(strings::MENU_MUTE));
        assert_eq!(
            label(true, true, ID_TOGGLE_MUTE),
            Some(strings::MENU_UNMUTE)
        );
        assert_eq!(
            label(true, true, ID_LEAVE_VOICE),
            Some(strings::MENU_LEAVE_VOICE)
        );
    }
}
