//! Tauri commands for rich presence (game detection).

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::command;

use crate::presence::{self, ProcessScanner};

/// Detected activity from process scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedActivity {
    /// Display name of the game/application.
    pub name: String,
    /// Type of activity (game, coding, listening, etc.).
    #[serde(rename = "type")]
    pub activity_type: String,
}

/// Global scanner state (lazy initialized).
static SCANNER: std::sync::OnceLock<Mutex<ProcessScanner>> = std::sync::OnceLock::new();

fn get_scanner() -> &'static Mutex<ProcessScanner> {
    SCANNER.get_or_init(|| Mutex::new(ProcessScanner::new()))
}

/// Scan running processes for known games/applications.
/// Returns the first detected game, or None if no known games are running.
#[command]
pub fn scan_processes() -> Option<DetectedActivity> {
    let mut scanner = get_scanner().lock().ok()?;
    scanner.scan().map(|game| DetectedActivity {
        name: game.name,
        activity_type: game.activity_type,
    })
}

/// Scan for all known games currently running.
/// Returns a list of all detected games.
#[command]
pub fn scan_all_processes() -> Vec<DetectedActivity> {
    let Ok(mut scanner) = get_scanner().lock() else {
        return Vec::new();
    };
    scanner
        .scan_all()
        .into_iter()
        .map(|game| DetectedActivity {
            name: game.name,
            activity_type: game.activity_type,
        })
        .collect()
}

/// Get list of all known games for settings UI.
#[command]
pub fn get_known_games() -> Vec<String> {
    let Ok(scanner) = get_scanner().lock() else {
        return Vec::new();
    };
    scanner.games_db.games.iter().map(|g| g.name.clone()).collect()
}

/// Enable or disable activity sharing.
#[command]
pub fn set_activity_sharing_enabled(enabled: bool) {
    presence::set_presence_enabled(enabled);
}

/// Check if activity sharing is enabled.
#[command]
pub fn is_activity_sharing_enabled() -> bool {
    presence::is_presence_enabled()
}
