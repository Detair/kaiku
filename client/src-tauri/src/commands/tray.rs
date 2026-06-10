//! Tray Commands
//!
//! Frontend bridge for the system tray unread badge.

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
