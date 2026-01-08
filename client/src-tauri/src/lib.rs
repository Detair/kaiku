//! VoiceChat Desktop Client Library
//!
//! Tauri backend for the desktop application.

mod audio;
mod commands;
mod crypto;
mod network;
mod webrtc;

use audio::AudioPipeline;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{mpsc, RwLock};
use webrtc::WebRtcClient;

use network::WebSocketManager;

/// Run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Initialize logging
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "vc_client=debug".into()),
                )
                .init();

            tracing::info!("VoiceChat Client starting");

            // Store app state
            app.manage(AppState::new());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::auth::login,
            commands::auth::logout,
            commands::auth::get_current_user,
            commands::auth::register,
            // Chat commands
            commands::chat::get_channels,
            commands::chat::get_messages,
            commands::chat::send_message,
            // Voice commands
            commands::voice::join_voice,
            commands::voice::leave_voice,
            commands::voice::set_mute,
            commands::voice::set_deafen,
            commands::voice::handle_voice_offer,
            commands::voice::handle_voice_ice_candidate,
            commands::voice::start_mic_test,
            commands::voice::stop_mic_test,
            commands::voice::get_mic_level,
            commands::voice::get_audio_devices,
            commands::voice::set_input_device,
            commands::voice::set_output_device,
            commands::voice::is_in_voice,
            commands::voice::get_voice_channel,
            // Settings commands
            commands::settings::get_settings,
            commands::settings::update_settings,
            // WebSocket commands
            commands::websocket::ws_connect,
            commands::websocket::ws_disconnect,
            commands::websocket::ws_status,
            commands::websocket::ws_subscribe,
            commands::websocket::ws_unsubscribe,
            commands::websocket::ws_typing,
            commands::websocket::ws_stop_typing,
            commands::websocket::ws_ping,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// User status.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Online,
    Away,
    Busy,
    #[default]
    Offline,
}

/// User profile (public info).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: UserStatus,
    pub email: Option<String>,
    pub mfa_enabled: bool,
}

/// Authentication state.
#[derive(Debug, Default)]
pub struct AuthState {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub user: Option<User>,
    pub server_url: Option<String>,
}

/// Voice connection state.
pub struct VoiceState {
    /// WebRTC client for peer connection.
    pub webrtc: WebRtcClient,
    /// Audio pipeline for capture/playback.
    pub audio: AudioPipeline,
    /// Current channel ID if connected.
    pub channel_id: Option<String>,
    /// Sender for encoded audio to WebRTC.
    pub audio_tx: Option<mpsc::Sender<Vec<u8>>>,
}

impl VoiceState {
    fn new() -> Result<Self, String> {
        let webrtc = WebRtcClient::new().map_err(|e| e.to_string())?;
        let audio = AudioPipeline::new().map_err(|e| e.to_string())?;
        Ok(Self {
            webrtc,
            audio,
            channel_id: None,
            audio_tx: None,
        })
    }
}

/// Application state shared across commands.
pub struct AppState {
    /// HTTP client for API requests.
    pub http: HttpClient,
    /// Authentication state.
    pub auth: Arc<RwLock<AuthState>>,
    /// WebSocket connection manager.
    pub websocket: Arc<RwLock<Option<WebSocketManager>>>,
    /// Voice state.
    pub voice: Arc<RwLock<Option<VoiceState>>>,
}

impl AppState {
    fn new() -> Self {
        let http = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            http,
            auth: Arc::new(RwLock::new(AuthState::default())),
            websocket: Arc::new(RwLock::new(None)),
            voice: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize voice state if not already initialized.
    pub async fn init_voice(&self) -> Result<(), String> {
        let mut voice = self.voice.write().await;
        if voice.is_none() {
            *voice = Some(VoiceState::new()?);
        }
        Ok(())
    }

    /// Get voice state, initializing if needed.
    pub async fn ensure_voice(&self) -> Result<(), String> {
        self.init_voice().await
    }

    /// Get the server URL if authenticated.
    pub async fn server_url(&self) -> Option<String> {
        self.auth.read().await.server_url.clone()
    }

    /// Get the access token if authenticated.
    pub async fn access_token(&self) -> Option<String> {
        self.auth.read().await.access_token.clone()
    }

    /// Check if authenticated.
    pub async fn is_authenticated(&self) -> bool {
        self.auth.read().await.access_token.is_some()
    }
}
