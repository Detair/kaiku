//! Tray Commands
//!
//! Frontend bridge for the system tray: unread badge and voice state.

use tauri::{command, AppHandle};

/// Update the system tray's unread indicator (tooltip everywhere, text
/// badge next to the icon where the platform supports tray titles).
///
/// Called by the frontend whenever the total unread count (guilds + DMs)
/// changes.
#[command]
pub fn tray_set_unread(count: u64, app: AppHandle) {
    crate::tray::update_unread_badge(&app, count);
}

/// Report the webview's voice state to the tray: drives the "Leave voice" /
/// "Mute" menu items and the close-to-tray voice notice.
///
/// `in_voice` is true from the start of a join until the call is left
/// (connecting counts, the mic may already be open).
#[command]
pub fn set_tray_voice_state(in_voice: bool, muted: bool, app: AppHandle) {
    crate::tray::set_voice_state(&app, in_voice, muted);
}
