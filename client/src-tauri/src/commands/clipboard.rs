//! Clipboard Protection Commands
//!
//! Secure clipboard operations with auto-clear, tamper detection, and audit logging.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::debug;

/// Context for what's being copied (affects sensitivity classification).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CopyContext {
    /// Recovery phrase for E2EE - CRITICAL
    RecoveryPhrase,
    /// Invite link - Sensitive
    InviteLink,
    /// General message content - Normal
    MessageContent,
    /// User ID - Normal
    UserId,
    /// Other content with custom label
    Other(String),
}

/// Sensitivity level determines auto-clear timeout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    /// Recovery phrases, E2EE keys - 60s (30s paranoid)
    Critical,
    /// Invite links, auth tokens - 120s (30s paranoid)
    Sensitive,
    /// Normal content - no auto-clear
    Normal,
}

impl Sensitivity {
    /// Get auto-clear timeout in seconds for standard mode.
    pub const fn standard_timeout_secs(&self) -> Option<u32> {
        match self {
            Self::Critical => Some(60),
            Self::Sensitive => Some(120),
            Self::Normal => None,
        }
    }

    /// Get auto-clear timeout in seconds for paranoid mode.
    pub const fn paranoid_timeout_secs(&self) -> Option<u32> {
        match self {
            Self::Critical | Self::Sensitive => Some(30),
            Self::Normal => None,
        }
    }
}

/// Classify copy context into sensitivity level.
const fn classify_context(context: &CopyContext) -> Sensitivity {
    match context {
        CopyContext::RecoveryPhrase => Sensitivity::Critical,
        CopyContext::InviteLink => Sensitivity::Sensitive,
        CopyContext::MessageContent | CopyContext::UserId | CopyContext::Other(_) => {
            Sensitivity::Normal
        }
    }
}

/// Protection level setting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProtectionLevel {
    /// No auto-clear, warn only on tamper
    Minimal,
    /// Standard timeouts, block on tamper
    #[default]
    Standard,
    /// Shorter timeouts, always show indicator
    Strict,
}

/// Clipboard settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSettings {
    pub protection_level: ProtectionLevel,
    pub paranoid_mode_enabled: bool,
    pub show_copy_toast: bool,
    pub show_status_indicator: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            protection_level: ProtectionLevel::Standard,
            paranoid_mode_enabled: false,
            show_copy_toast: true,
            show_status_indicator: true,
        }
    }
}

/// Pending clipboard clear information.
#[derive(Debug)]
struct PendingClear {
    #[allow(dead_code)]
    content_hash: [u8; 32],
    clear_at: Instant,
    context: CopyContext,
    sensitivity: Sensitivity,
    extensions_used: u8,
}

/// Result of a copy operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyResult {
    pub success: bool,
    pub auto_clear_in_secs: Option<u32>,
    pub sensitivity: Sensitivity,
}

/// Result of a paste operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteResult {
    pub content: String,
    pub tampered: bool,
    pub external: bool,
    pub context: Option<CopyContext>,
}

/// Clipboard errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardError {
    AccessDenied,
    TamperDetected,
    Cleared,
    MaxExtensionsReached,
    SystemError(String),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessDenied => write!(f, "Clipboard access denied"),
            Self::TamperDetected => write!(f, "Clipboard content was modified"),
            Self::Cleared => write!(f, "Clipboard was cleared"),
            Self::MaxExtensionsReached => write!(f, "Maximum timeout extensions reached"),
            Self::SystemError(msg) => write!(f, "System error: {msg}"),
        }
    }
}

/// Status event emitted on clipboard state changes.
#[derive(Debug, Clone, Serialize)]
pub struct ClipboardStatusEvent {
    pub has_sensitive_content: bool,
    pub clear_in_secs: Option<u32>,
    pub context: Option<CopyContext>,
    pub sensitivity: Option<Sensitivity>,
}

/// Clipboard guard service managing secure clipboard operations.
pub struct ClipboardGuard {
    /// Current pending clear task
    pending_clear: RwLock<Option<PendingClear>>,
    /// Known content hash (for tamper detection)
    known_hash: RwLock<Option<[u8; 32]>>,
    /// Known context
    known_context: RwLock<Option<CopyContext>>,
    /// Settings
    settings: RwLock<ClipboardSettings>,
}

impl ClipboardGuard {
    /// Create a new clipboard guard.
    pub fn new() -> Self {
        Self {
            pending_clear: RwLock::new(None),
            known_hash: RwLock::new(None),
            known_context: RwLock::new(None),
            settings: RwLock::new(ClipboardSettings::default()),
        }
    }

    /// Compute SHA-256 hash of content.
    fn hash_content(content: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hasher.finalize().into()
    }

    /// Get auto-clear timeout based on settings.
    async fn get_timeout(&self, sensitivity: Sensitivity) -> Option<u32> {
        let settings = self.settings.read().await;

        if settings.protection_level == ProtectionLevel::Minimal {
            return None;
        }

        if settings.paranoid_mode_enabled {
            sensitivity.paranoid_timeout_secs()
        } else if settings.protection_level == ProtectionLevel::Strict {
            // Strict mode uses paranoid timeouts
            sensitivity.paranoid_timeout_secs()
        } else {
            sensitivity.standard_timeout_secs()
        }
    }

    /// Copy content to clipboard with protection.
    pub async fn copy(
        &self,
        content: &str,
        context: CopyContext,
        app: &AppHandle,
    ) -> Result<CopyResult, ClipboardError> {
        let sensitivity = classify_context(&context);
        let timeout_secs = self.get_timeout(sensitivity).await;

        // Write to system clipboard
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        clipboard
            .set_text(content)
            .map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        // Store hash for tamper detection
        let hash = Self::hash_content(content);
        *self.known_hash.write().await = Some(hash);
        *self.known_context.write().await = Some(context.clone());

        // Schedule auto-clear if needed
        if let Some(secs) = timeout_secs {
            let clear_at = Instant::now() + Duration::from_secs(u64::from(secs));
            *self.pending_clear.write().await = Some(PendingClear {
                content_hash: hash,
                clear_at,
                context: context.clone(),
                sensitivity,
                extensions_used: 0,
            });

            // Start background clear task
            self.start_clear_task(app.clone(), secs);
        } else {
            *self.pending_clear.write().await = None;
        }

        // Emit status event
        let _ = app.emit(
            "clipboard-status",
            ClipboardStatusEvent {
                has_sensitive_content: sensitivity != Sensitivity::Normal,
                clear_in_secs: timeout_secs,
                context: Some(context),
                sensitivity: Some(sensitivity),
            },
        );

        debug!(?sensitivity, timeout_secs, "Clipboard copy with protection");

        Ok(CopyResult {
            success: true,
            auto_clear_in_secs: timeout_secs,
            sensitivity,
        })
    }

    /// Start background task to clear clipboard after timeout.
    fn start_clear_task(&self, app: AppHandle, secs: u32) {
        let guard = app.state::<Arc<Self>>().inner().clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(u64::from(secs))).await;

            // Check if we should still clear
            let pending = guard.pending_clear.read().await;
            if let Some(ref clear) = *pending {
                if clear.clear_at <= Instant::now() {
                    drop(pending);

                    // Verify clipboard still has our content
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(current) = clipboard.get_text() {
                            let current_hash = Self::hash_content(&current);
                            let known: Option<[u8; 32]> = *guard.known_hash.read().await;

                            if known.as_ref() == Some(&current_hash) {
                                // Clear clipboard
                                let _ = clipboard.clear();
                                debug!("Auto-cleared clipboard after timeout");

                                // Clear our state
                                *guard.pending_clear.write().await = None;
                                *guard.known_hash.write().await = None;
                                *guard.known_context.write().await = None;

                                // Emit cleared event
                                let _ = app.emit(
                                    "clipboard-status",
                                    ClipboardStatusEvent {
                                        has_sensitive_content: false,
                                        clear_in_secs: None,
                                        context: None,
                                        sensitivity: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    /// Paste from clipboard with tamper detection.
    pub async fn paste(&self) -> Result<PasteResult, ClipboardError> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        let content = clipboard
            .get_text()
            .map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        let current_hash = Self::hash_content(&content);
        let known_hash = self.known_hash.read().await;
        let known_context = self.known_context.read().await;

        let (tampered, external) = match *known_hash {
            Some(ref hash) => {
                if *hash == current_hash {
                    (false, false)
                } else {
                    // Content changed - could be tamper or user copied something else
                    (true, true)
                }
            }
            None => {
                // We didn't copy this - it's external
                (false, true)
            }
        };

        Ok(PasteResult {
            content,
            tampered,
            external,
            context: if external {
                None
            } else {
                known_context.clone()
            },
        })
    }

    /// Clear clipboard immediately.
    pub async fn clear(&self, app: &AppHandle) -> Result<(), ClipboardError> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        clipboard
            .clear()
            .map_err(|e| ClipboardError::SystemError(e.to_string()))?;

        // Clear our state
        *self.pending_clear.write().await = None;
        *self.known_hash.write().await = None;
        *self.known_context.write().await = None;

        // Emit cleared event
        let _ = app.emit(
            "clipboard-status",
            ClipboardStatusEvent {
                has_sensitive_content: false,
                clear_in_secs: None,
                context: None,
                sensitivity: None,
            },
        );

        debug!("Clipboard cleared manually");
        Ok(())
    }

    /// Extend the auto-clear timeout.
    pub async fn extend_timeout(
        &self,
        additional_secs: u32,
        app: &AppHandle,
    ) -> Result<u32, ClipboardError> {
        let mut pending = self.pending_clear.write().await;

        let clear = pending
            .as_mut()
            .ok_or_else(|| ClipboardError::SystemError("No pending clear".to_string()))?;

        // Max 2 extensions
        if clear.extensions_used >= 2 {
            return Err(ClipboardError::MaxExtensionsReached);
        }

        clear.extensions_used += 1;
        clear.clear_at = Instant::now() + Duration::from_secs(u64::from(additional_secs));

        let new_secs = additional_secs;

        // Restart clear task
        drop(pending);
        self.start_clear_task(app.clone(), new_secs);

        // Emit updated status
        let pending = self.pending_clear.read().await;
        if let Some(ref clear) = *pending {
            let _ = app.emit(
                "clipboard-status",
                ClipboardStatusEvent {
                    has_sensitive_content: true,
                    clear_in_secs: Some(new_secs),
                    context: Some(clear.context.clone()),
                    sensitivity: Some(clear.sensitivity),
                },
            );
        }

        Ok(new_secs)
    }

    /// Get remaining seconds until auto-clear.
    pub async fn get_remaining_secs(&self) -> Option<u32> {
        let pending = self.pending_clear.read().await;
        pending.as_ref().map(|clear| {
            let remaining = clear.clear_at.saturating_duration_since(Instant::now());
            remaining.as_secs() as u32
        })
    }

    /// Update settings.
    pub async fn update_settings(&self, settings: ClipboardSettings) {
        *self.settings.write().await = settings;
    }

    /// Get current settings.
    pub async fn get_settings(&self) -> ClipboardSettings {
        self.settings.read().await.clone()
    }
}

impl Default for ClipboardGuard {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Copy content to clipboard securely.
#[tauri::command]
pub async fn secure_copy(
    content: String,
    context: CopyContext,
    guard: State<'_, Arc<ClipboardGuard>>,
    app: AppHandle,
) -> Result<CopyResult, String> {
    guard
        .copy(&content, context, &app)
        .await
        .map_err(|e| e.to_string())
}

/// Paste from clipboard with tamper detection.
#[tauri::command]
pub async fn secure_paste(guard: State<'_, Arc<ClipboardGuard>>) -> Result<PasteResult, String> {
    guard.paste().await.map_err(|e| e.to_string())
}

/// Clear clipboard immediately.
#[tauri::command]
pub async fn clear_clipboard(
    guard: State<'_, Arc<ClipboardGuard>>,
    app: AppHandle,
) -> Result<(), String> {
    guard.clear(&app).await.map_err(|e| e.to_string())
}

/// Extend the auto-clear timeout.
#[tauri::command]
pub async fn extend_clipboard_timeout(
    additional_secs: u32,
    guard: State<'_, Arc<ClipboardGuard>>,
    app: AppHandle,
) -> Result<u32, String> {
    guard
        .extend_timeout(additional_secs, &app)
        .await
        .map_err(|e| e.to_string())
}

/// Get remaining seconds until auto-clear.
#[tauri::command]
pub async fn get_clipboard_status(
    guard: State<'_, Arc<ClipboardGuard>>,
) -> Result<ClipboardStatusEvent, String> {
    let remaining = guard.get_remaining_secs().await;
    let pending = guard.pending_clear.read().await;

    Ok(ClipboardStatusEvent {
        has_sensitive_content: pending.is_some(),
        clear_in_secs: remaining,
        context: pending.as_ref().map(|c| c.context.clone()),
        sensitivity: pending.as_ref().map(|c| c.sensitivity),
    })
}

/// Update clipboard protection settings.
#[tauri::command]
pub async fn update_clipboard_settings(
    settings: ClipboardSettings,
    guard: State<'_, Arc<ClipboardGuard>>,
) -> Result<(), String> {
    guard.update_settings(settings).await;
    Ok(())
}

/// Get current clipboard protection settings.
#[tauri::command]
pub async fn get_clipboard_settings(
    guard: State<'_, Arc<ClipboardGuard>>,
) -> Result<ClipboardSettings, String> {
    Ok(guard.get_settings().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitivity_classification() {
        assert_eq!(
            classify_context(&CopyContext::RecoveryPhrase),
            Sensitivity::Critical
        );
        assert_eq!(
            classify_context(&CopyContext::InviteLink),
            Sensitivity::Sensitive
        );
        assert_eq!(
            classify_context(&CopyContext::MessageContent),
            Sensitivity::Normal
        );
        assert_eq!(classify_context(&CopyContext::UserId), Sensitivity::Normal);
        assert_eq!(
            classify_context(&CopyContext::Other("test".to_string())),
            Sensitivity::Normal
        );
    }

    #[test]
    fn test_sensitivity_timeouts() {
        // Standard mode
        assert_eq!(Sensitivity::Critical.standard_timeout_secs(), Some(60));
        assert_eq!(Sensitivity::Sensitive.standard_timeout_secs(), Some(120));
        assert_eq!(Sensitivity::Normal.standard_timeout_secs(), None);

        // Paranoid mode
        assert_eq!(Sensitivity::Critical.paranoid_timeout_secs(), Some(30));
        assert_eq!(Sensitivity::Sensitive.paranoid_timeout_secs(), Some(30));
        assert_eq!(Sensitivity::Normal.paranoid_timeout_secs(), None);
    }

    #[test]
    fn test_hash_content() {
        let hash1 = ClipboardGuard::hash_content("test");
        let hash2 = ClipboardGuard::hash_content("test");
        let hash3 = ClipboardGuard::hash_content("different");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_default_settings() {
        let settings = ClipboardSettings::default();
        assert_eq!(settings.protection_level, ProtectionLevel::Standard);
        assert!(!settings.paranoid_mode_enabled);
        assert!(settings.show_copy_toast);
        assert!(settings.show_status_indicator);
    }
}
