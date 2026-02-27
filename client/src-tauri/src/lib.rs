//! `VoiceChat` Desktop Client Library
//!
//! Tauri backend for the desktop application.

mod audio;
mod capture;
mod commands;
mod crypto;
mod network;
mod presence;
mod video;
mod webrtc;

use std::sync::Arc;

use audio::AudioHandle;
use commands::clipboard::ClipboardGuard;
use commands::screen_share::ScreenSharePipeline;
use commands::settings::UiState;
use commands::webcam::WebcamPipeline;
use network::WebSocketManager;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::{mpsc, Mutex, RwLock};
use webrtc::WebRtcClient;

/// Run the Tauri application.
pub fn run() {
    // Initialize Sentry only when DSN is configured
    let _sentry_guard = std::env::var("SENTRY_DSN_CLIENT")
        .ok()
        .filter(|dsn| !dsn.is_empty())
        .map(|dsn| {
            sentry::init((
                dsn,
                sentry::ClientOptions {
                    release: sentry::release_name!(),
                    environment: Some(
                        std::env::var("APP_ENV")
                            .unwrap_or_else(|_| "development".to_string())
                            .into(),
                    ),
                    sample_rate: 1.0,
                    traces_sample_rate: 0.05,
                    send_default_pii: false,
                    auto_session_tracking: true,
                    ..Default::default()
                },
            ))
        });

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

            // Store clipboard guard
            app.manage(Arc::new(ClipboardGuard::new()));

            // Start presence polling service
            presence::start_presence_service(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::auth::login,
            commands::auth::logout,
            commands::auth::get_current_user,
            commands::auth::get_auth_info,
            commands::auth::register,
            commands::auth::oidc_authorize,
            commands::auth::mfa_setup,
            commands::auth::mfa_verify,
            commands::auth::mfa_disable,
            commands::auth::mfa_generate_backup_codes,
            commands::auth::mfa_backup_code_count,
            // Chat commands
            commands::chat::get_channels,
            commands::chat::get_messages,
            commands::chat::send_message,
            commands::chat::get_thread_replies,
            commands::chat::send_thread_reply,
            commands::chat::mark_thread_read,
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
            // Screen share commands
            commands::screen_share::enumerate_capture_sources,
            commands::screen_share::start_screen_share,
            commands::screen_share::stop_screen_share,
            commands::screen_share::get_screen_share_status,
            // Webcam commands
            commands::webcam::start_webcam,
            commands::webcam::stop_webcam,
            commands::webcam::enumerate_webcam_devices_cmd,
            // Settings commands
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_ui_state,
            commands::settings::update_category_collapse,
            // WebSocket commands
            commands::websocket::ws_connect,
            commands::websocket::ws_disconnect,
            commands::websocket::ws_status,
            commands::websocket::ws_subscribe,
            commands::websocket::ws_unsubscribe,
            commands::websocket::ws_typing,
            commands::websocket::ws_stop_typing,
            commands::websocket::ws_ping,
            commands::websocket::ws_send_activity,
            // Pages commands
            commands::pages::list_platform_pages,
            commands::pages::get_platform_page,
            commands::pages::create_platform_page,
            commands::pages::update_platform_page,
            commands::pages::delete_platform_page,
            commands::pages::reorder_platform_pages,
            commands::pages::list_guild_pages,
            commands::pages::get_guild_page,
            commands::pages::create_guild_page,
            commands::pages::update_guild_page,
            commands::pages::delete_guild_page,
            commands::pages::reorder_guild_pages,
            commands::pages::accept_page,
            commands::pages::get_pending_acceptance,
            commands::pages::list_page_revisions,
            commands::pages::get_page_revision,
            commands::pages::restore_page_revision,
            commands::pages::list_page_categories,
            commands::pages::create_page_category,
            commands::pages::update_page_category,
            commands::pages::delete_page_category,
            commands::pages::reorder_page_categories,
            // Role commands
            commands::roles::get_guild_roles,
            commands::roles::create_guild_role,
            commands::roles::update_guild_role,
            commands::roles::delete_guild_role,
            commands::roles::get_guild_member_roles,
            commands::roles::assign_member_role,
            commands::roles::remove_member_role,
            commands::roles::get_channel_overrides,
            commands::roles::set_channel_override,
            commands::roles::delete_channel_override,
            // Admin commands
            commands::admin::check_admin_status,
            commands::admin::get_admin_stats,
            commands::admin::admin_list_users,
            commands::admin::admin_list_guilds,
            commands::admin::admin_get_audit_log,
            commands::admin::admin_elevate,
            commands::admin::admin_de_elevate,
            commands::admin::admin_ban_user,
            commands::admin::admin_unban_user,
            commands::admin::admin_delete_user,
            commands::admin::admin_suspend_guild,
            commands::admin::admin_unsuspend_guild,
            commands::admin::admin_delete_guild,
            // Crypto commands
            commands::crypto::get_server_settings,
            commands::crypto::get_backup_status,
            commands::crypto::generate_recovery_key,
            commands::crypto::create_backup,
            commands::crypto::restore_backup,
            // E2EE commands
            commands::crypto::get_e2ee_status,
            commands::crypto::init_e2ee,
            commands::crypto::encrypt_message,
            commands::crypto::decrypt_message,
            commands::crypto::mark_prekeys_published,
            commands::crypto::generate_prekeys,
            commands::crypto::needs_prekey_upload,
            commands::crypto::get_our_curve25519_key,
            // Megolm commands
            commands::crypto::create_megolm_session,
            commands::crypto::encrypt_group_message,
            commands::crypto::add_inbound_group_session,
            commands::crypto::decrypt_group_message,
            // Presence commands
            commands::presence::scan_processes,
            commands::presence::scan_all_processes,
            commands::presence::get_known_games,
            commands::presence::set_activity_sharing_enabled,
            commands::presence::is_activity_sharing_enabled,
            // Sound commands
            commands::sound::play_sound,
            commands::sound::get_available_sounds,
            // Clipboard commands
            commands::clipboard::secure_copy,
            commands::clipboard::secure_paste,
            commands::clipboard::clear_clipboard,
            commands::clipboard::extend_clipboard_timeout,
            commands::clipboard::get_clipboard_status,
            commands::clipboard::update_clipboard_settings,
            commands::clipboard::get_clipboard_settings,
            // Call commands
            commands::calls::start_dm_call,
            commands::calls::join_dm_call,
            commands::calls::decline_dm_call,
            commands::calls::leave_dm_call,
            commands::calls::get_dm_call,
            // Preferences commands
            commands::preferences::fetch_preferences,
            commands::preferences::update_preferences,
            // Pins commands
            commands::pins::fetch_pins,
            commands::pins::create_pin,
            commands::pins::update_pin,
            commands::pins::delete_pin,
            commands::pins::reorder_pins,
            // Favorites commands
            commands::favorites::fetch_favorites,
            commands::favorites::add_favorite,
            commands::favorites::remove_favorite,
            commands::favorites::reorder_favorite_channels,
            commands::favorites::reorder_favorite_guilds,
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
    /// Audio handle for capture/playback (Send + Sync).
    pub audio: AudioHandle,
    /// Current channel ID if connected.
    pub channel_id: Option<String>,
    /// Sender for encoded audio to WebRTC.
    pub audio_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Active screen share pipeline, if any.
    pub screen_share: Option<ScreenSharePipeline>,
    /// Active webcam pipeline, if any.
    pub webcam: Option<WebcamPipeline>,
}

impl VoiceState {
    fn new() -> Result<Self, String> {
        let webrtc = WebRtcClient::new().map_err(|e| e.to_string())?;
        let audio = AudioHandle::new().map_err(|e| e.to_string())?;
        Ok(Self {
            webrtc,
            audio,
            channel_id: None,
            audio_tx: None,
            screen_share: None,
            webcam: None,
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
    /// E2EE crypto manager.
    /// Uses `Mutex` instead of `RwLock` because `rusqlite::Connection` is `Send` but not `Sync`.
    pub crypto: Arc<Mutex<Option<crypto::CryptoManager>>>,
    /// Cached UI state (category collapse). Lazy-loaded from disk on first access.
    pub ui_state: Arc<Mutex<Option<UiState>>>,
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
            crypto: Arc::new(Mutex::new(None)),
            ui_state: Arc::new(Mutex::new(None)),
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
