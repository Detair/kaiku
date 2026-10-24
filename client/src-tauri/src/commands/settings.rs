//! Settings Commands
//!
//! Persistent settings and UI state stored as JSON files in the app data directory.
//! Settings I/O uses `spawn_blocking` to avoid blocking the async runtime.
//! UI state is cached in memory behind a `Mutex` to serialize read-modify-write operations.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{command, Manager};

/// Tester override for the server address (`KAIKU_SERVER_URL`).
///
/// Lets CI-built packages point at a dev stack. Unset (or blank) for normal users.
#[command]
pub fn get_default_server_url() -> Option<String> {
    std::env::var("KAIKU_SERVER_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

// ============================================================================
// Settings Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AudioSettings {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_volume: f32,
    pub output_volume: f32,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_volume: 100.0,
            output_volume: 100.0,
            noise_suppression: true,
            echo_cancellation: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct VoiceSettings {
    pub push_to_talk: bool,
    pub push_to_talk_key: Option<String>,
    pub push_to_talk_release_delay: u32,
    pub push_to_mute: bool,
    pub push_to_mute_key: Option<String>,
    pub push_to_mute_release_delay: u32,
    pub voice_activity_detection: bool,
    pub vad_threshold: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            push_to_talk: false,
            push_to_talk_key: None,
            push_to_talk_release_delay: 200,
            push_to_mute: false,
            push_to_mute_key: None,
            push_to_mute_release_delay: 200,
            voice_activity_detection: true,
            vad_threshold: 0.5,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub voice: VoiceSettings,
    pub theme: String,
    pub notifications_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            voice: VoiceSettings::default(),
            theme: "dark".into(),
            notifications_enabled: true,
        }
    }
}

impl Settings {
    /// Clamp values to valid ranges and fix inconsistent state.
    pub fn validated(mut self) -> Self {
        self.audio.input_volume = self.audio.input_volume.clamp(0.0, 150.0);
        self.audio.output_volume = self.audio.output_volume.clamp(0.0, 150.0);
        self.voice.vad_threshold = self.voice.vad_threshold.clamp(0.0, 1.0);
        if !matches!(self.theme.as_str(), "dark" | "light") {
            self.theme = "dark".into();
        }
        // PTT without a key binding is unusable — fall back to VAD
        if self.voice.push_to_talk && self.voice.push_to_talk_key.is_none() {
            self.voice.push_to_talk = false;
            self.voice.voice_activity_detection = true;
        }
        // PTM without a key binding is unusable — disable it
        if self.voice.push_to_mute && self.voice.push_to_mute_key.is_none() {
            self.voice.push_to_mute = false;
        }
        // PTT and PTM keys must differ
        if self.voice.push_to_talk
            && self.voice.push_to_mute
            && self.voice.push_to_talk_key == self.voice.push_to_mute_key
        {
            self.voice.push_to_mute = false;
            self.voice.push_to_mute_key = None;
        }
        // Clamp release delays
        self.voice.push_to_talk_release_delay = self.voice.push_to_talk_release_delay.min(1000);
        self.voice.push_to_mute_release_delay = self.voice.push_to_mute_release_delay.min(1000);
        self
    }
}

// ============================================================================
// UI State Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct UiState {
    pub category_collapse: HashMap<String, bool>,
}

// ============================================================================
// File Persistence Helpers
// ============================================================================

fn get_settings_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;

    Ok(app_data_dir.join("settings.json"))
}

fn get_ui_state_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;

    Ok(app_data_dir.join("ui_state.json"))
}

fn load_settings_from_file(path: &PathBuf) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!("Corrupt settings file, using defaults: {e}");
            Settings::default()
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            tracing::warn!("Failed to read settings file, using defaults: {e}");
            Settings::default()
        }
    }
}

fn save_settings_to_file(path: &PathBuf, settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write settings file: {e}"))
}

fn load_ui_state_from_file(path: &PathBuf) -> UiState {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!("Corrupt UI state file, using defaults: {e}");
            UiState::default()
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => UiState::default(),
        Err(e) => {
            tracing::warn!("Failed to read UI state file, using defaults: {e}");
            UiState::default()
        }
    }
}

fn save_ui_state_to_file(path: &PathBuf, state: &UiState) -> Result<(), String> {
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize UI state: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write UI state file: {e}"))
}

// ============================================================================
// Settings Commands
// ============================================================================

#[command]
pub async fn get_settings(app_handle: tauri::AppHandle) -> Result<Settings, String> {
    let path = get_settings_path(&app_handle)?;
    tokio::task::spawn_blocking(move || load_settings_from_file(&path).validated())
        .await
        .map_err(|e| format!("Task join error: {e}"))
}

#[command]
pub async fn update_settings(
    app_handle: tauri::AppHandle,
    settings: Settings,
) -> Result<(), String> {
    let path = get_settings_path(&app_handle)?;
    let settings = settings.validated();
    tokio::task::spawn_blocking(move || save_settings_to_file(&path, &settings))
        .await
        .map_err(|e| format!("Task join error: {e}"))?
}

// ============================================================================
// UI State Commands
// ============================================================================

/// Load `UiState` into the in-memory cache if not already loaded.
async fn ensure_ui_state_loaded(
    app_handle: &tauri::AppHandle,
    cache: &mut Option<UiState>,
) -> Result<(), String> {
    if cache.is_none() {
        let path = get_ui_state_path(app_handle)?;
        *cache = Some(load_ui_state_from_file(&path));
    }
    Ok(())
}

#[command]
pub async fn get_ui_state(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<UiState, String> {
    let mut guard = state.ui_state.lock().await;
    ensure_ui_state_loaded(&app_handle, &mut guard).await?;
    Ok(guard.as_ref().unwrap().clone())
}

#[command]
pub async fn update_category_collapse(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    category_id: String,
    collapsed: bool,
) -> Result<(), String> {
    let path = get_ui_state_path(&app_handle)?;
    let mut guard = state.ui_state.lock().await;
    ensure_ui_state_loaded(&app_handle, &mut guard).await?;

    let ui_state = guard.as_mut().unwrap();
    ui_state.category_collapse.insert(category_id, collapsed);

    // Write to disk while holding the lock to prevent interleaving.
    // File is < 1KB so blocking duration is negligible.
    save_ui_state_to_file(&path, ui_state)
}

#[cfg(test)]
mod default_server_url_tests {
    use std::sync::Mutex;

    use super::*;

    /// Serializes tests that mutate the process environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_server_url_reads_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("KAIKU_SERVER_URL", "https://dev.example.org");
        assert_eq!(
            get_default_server_url().as_deref(),
            Some("https://dev.example.org")
        );
        std::env::set_var("KAIKU_SERVER_URL", "   ");
        assert_eq!(get_default_server_url(), None);
        std::env::remove_var("KAIKU_SERVER_URL");
        assert_eq!(get_default_server_url(), None);
    }
}
