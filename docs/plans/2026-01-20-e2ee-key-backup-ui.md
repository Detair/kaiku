# E2EE Key Backup UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the UI for E2EE key backup - recovery key modal, security settings, and Tauri commands for key management.

**Architecture:** Frontend modal shown after registration displays the recovery key with copy/download options. Security settings tab shows backup status. Tauri commands handle key generation and backup operations via the existing server API. Server config `REQUIRE_E2EE_SETUP` can make this mandatory.

**Tech Stack:** Solid.js (frontend), Tauri commands (Rust), vc-crypto (shared), existing server API endpoints.

---

## Task 1: Add Server Config for E2EE Requirement

**Files:**
- Modify: `server/src/config.rs`

**Step 1: Add `require_e2ee_setup` field to Config**

```rust
// In Config struct, add after mfa_encryption_key:

    /// Whether E2EE setup is required before using the app (default: false)
    pub require_e2ee_setup: bool,
```

**Step 2: Load from environment in `from_env()`**

```rust
// In from_env(), add after mfa_encryption_key:

            require_e2ee_setup: env::var("REQUIRE_E2EE_SETUP")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
```

**Step 3: Add to `default_for_test()`**

```rust
// In default_for_test(), add:
            require_e2ee_setup: false,
```

**Step 4: Commit**

```bash
git add server/src/config.rs
git commit -m "feat(config): add REQUIRE_E2EE_SETUP option"
```

---

## Task 2: Add Server Settings API Endpoint

**Files:**
- Modify: `server/src/api/mod.rs`
- Create: `server/src/api/settings.rs`

**Step 1: Create settings.rs with server settings endpoint**

```rust
//! Server Settings API
//!
//! Public endpoint for retrieving server configuration that clients need.

use axum::{extract::State, Json};
use serde::Serialize;

use crate::api::AppState;

/// Public server settings response.
#[derive(Debug, Serialize)]
pub struct ServerSettingsResponse {
    /// Whether E2EE setup is required before using the app.
    pub require_e2ee_setup: bool,
    /// Whether OIDC login is available.
    pub oidc_enabled: bool,
}

/// Get server settings (public endpoint).
///
/// GET /api/settings
pub async fn get_server_settings(
    State(state): State<AppState>,
) -> Json<ServerSettingsResponse> {
    Json(ServerSettingsResponse {
        require_e2ee_setup: state.config.require_e2ee_setup,
        oidc_enabled: state.config.has_oidc(),
    })
}
```

**Step 2: Add route in mod.rs**

```rust
// Add to imports:
mod settings;

// Add route in router() after other routes:
        .route("/api/settings", get(settings::get_server_settings))
```

**Step 3: Commit**

```bash
git add server/src/api/settings.rs server/src/api/mod.rs
git commit -m "feat(api): add /api/settings endpoint for server config"
```

---

## Task 3: Add Backup Status Check Endpoint

**Files:**
- Modify: `server/src/crypto/handlers.rs`
- Modify: `server/src/crypto/mod.rs`

**Step 1: Add backup status response type in handlers.rs**

```rust
/// Response for backup status check.
#[derive(Debug, Serialize)]
pub struct BackupStatusResponse {
    /// Whether a backup exists.
    pub has_backup: bool,
    /// When the backup was created (if exists).
    pub backup_created_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Backup version (if exists).
    pub version: Option<i32>,
}
```

**Step 2: Add backup status handler in handlers.rs**

```rust
/// Check if user has a key backup.
///
/// GET /api/keys/backup/status
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_backup_status(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<BackupStatusResponse>, AuthError> {
    let backup = sqlx::query_as::<_, BackupStatusRow>(
        "SELECT version, created_at FROM key_backups WHERE user_id = $1",
    )
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?;

    Ok(Json(match backup {
        Some(b) => BackupStatusResponse {
            has_backup: true,
            backup_created_at: Some(b.created_at),
            version: Some(b.version),
        },
        None => BackupStatusResponse {
            has_backup: false,
            backup_created_at: None,
            version: None,
        },
    }))
}

#[derive(Debug, FromRow)]
struct BackupStatusRow {
    version: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}
```

**Step 3: Add route in crypto/mod.rs**

```rust
// In router(), add:
        .route("/backup/status", get(handlers::get_backup_status))
```

**Step 4: Commit**

```bash
git add server/src/crypto/handlers.rs server/src/crypto/mod.rs
git commit -m "feat(api): add /api/keys/backup/status endpoint"
```

---

## Task 4: Add Tauri Crypto Commands Module

**Files:**
- Create: `client/src-tauri/src/commands/crypto.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`

**Step 1: Create crypto.rs with types**

```rust
//! E2EE Key Management Commands

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{error, info};

use crate::AppState;

/// Recovery key formatted for display (4-char chunks).
#[derive(Debug, Serialize)]
pub struct RecoveryKeyDisplay {
    /// Full key in Base58 (for copy/download).
    pub full_key: String,
    /// Key split into 4-char chunks for display.
    pub chunks: Vec<String>,
}

/// Backup status from server.
#[derive(Debug, Deserialize, Serialize)]
pub struct BackupStatus {
    pub has_backup: bool,
    pub backup_created_at: Option<String>,
    pub version: Option<i32>,
}

/// Server settings.
#[derive(Debug, Deserialize, Serialize)]
pub struct ServerSettings {
    pub require_e2ee_setup: bool,
    pub oidc_enabled: bool,
}
```

**Step 2: Add get_server_settings command**

```rust
/// Get server settings.
#[command]
pub async fn get_server_settings(state: State<'_, AppState>) -> Result<ServerSettings, String> {
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;

    let response = state
        .http
        .get(format!("{server_url}/api/settings"))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    response
        .json::<ServerSettings>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}
```

**Step 3: Add get_backup_status command**

```rust
/// Get backup status for current user.
#[command]
pub async fn get_backup_status(state: State<'_, AppState>) -> Result<BackupStatus, String> {
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .get(format!("{server_url}/api/keys/backup/status"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    response
        .json::<BackupStatus>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}
```

**Step 4: Add module to mod.rs**

```rust
// Add to commands/mod.rs:
pub mod crypto;
```

**Step 5: Commit**

```bash
git add client/src-tauri/src/commands/crypto.rs client/src-tauri/src/commands/mod.rs
git commit -m "feat(tauri): add crypto commands for server settings and backup status"
```

---

## Task 5: Add Recovery Key Generation Command

**Files:**
- Modify: `client/src-tauri/src/commands/crypto.rs`
- Modify: `client/src-tauri/Cargo.toml`

**Step 1: Add vc-crypto dependency to Cargo.toml**

```toml
# In [dependencies], add:
vc-crypto = { path = "../../shared/vc-crypto" }
```

**Step 2: Add generate_recovery_key command**

```rust
// Add import at top:
use vc_crypto::recovery::RecoveryKey;

/// Generate a new recovery key and return it for display.
///
/// The key is NOT stored - the UI must prompt user to save it,
/// then call create_backup to actually store the encrypted backup.
#[command]
pub async fn generate_recovery_key() -> Result<RecoveryKeyDisplay, String> {
    let key = RecoveryKey::generate();
    let full_key = key.to_base58();

    // Split into 4-char chunks
    let chunks: Vec<String> = full_key
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect();

    info!("Generated new recovery key");

    Ok(RecoveryKeyDisplay { full_key, chunks })
}
```

**Step 3: Commit**

```bash
git add client/src-tauri/src/commands/crypto.rs client/src-tauri/Cargo.toml
git commit -m "feat(tauri): add generate_recovery_key command"
```

---

## Task 6: Add Create Backup Command

**Files:**
- Modify: `client/src-tauri/src/commands/crypto.rs`

**Step 1: Add backup request type**

```rust
#[derive(Debug, Serialize)]
struct UploadBackupRequest {
    salt: String,
    nonce: String,
    ciphertext: String,
    version: i32,
}
```

**Step 2: Add create_backup command**

```rust
use base64::{engine::general_purpose::STANDARD, Engine};
use vc_crypto::recovery::EncryptedBackup;

/// Create and upload an encrypted backup of the user's keys.
///
/// Takes the recovery key (Base58) and the data to backup (JSON string).
/// Encrypts locally, then uploads to server.
#[command]
pub async fn create_backup(
    state: State<'_, AppState>,
    recovery_key: String,
    backup_data: String,
) -> Result<(), String> {
    // Parse recovery key
    let key = RecoveryKey::from_base58(&recovery_key)
        .map_err(|e| format!("Invalid recovery key: {e}"))?;

    // Encrypt the backup data
    let encrypted = EncryptedBackup::encrypt(backup_data.as_bytes(), &key)
        .map_err(|e| format!("Encryption failed: {e}"))?;

    // Prepare request
    let request = UploadBackupRequest {
        salt: STANDARD.encode(&encrypted.salt),
        nonce: STANDARD.encode(&encrypted.nonce),
        ciphertext: STANDARD.encode(&encrypted.ciphertext),
        version: 1,
    };

    // Upload to server
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .post(format!("{server_url}/api/keys/backup"))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("Backup upload failed: {}", body);
        return Err(format!("Server error: {body}"));
    }

    info!("Backup uploaded successfully");
    Ok(())
}
```

**Step 3: Commit**

```bash
git add client/src-tauri/src/commands/crypto.rs
git commit -m "feat(tauri): add create_backup command"
```

---

## Task 7: Add Restore Backup Command

**Files:**
- Modify: `client/src-tauri/src/commands/crypto.rs`

**Step 1: Add backup response type**

```rust
#[derive(Debug, Deserialize)]
struct BackupResponse {
    salt: String,
    nonce: String,
    ciphertext: String,
    version: i32,
    created_at: String,
}
```

**Step 2: Add restore_backup command**

```rust
/// Download and decrypt a backup using the recovery key.
///
/// Returns the decrypted backup data as a JSON string.
#[command]
pub async fn restore_backup(
    state: State<'_, AppState>,
    recovery_key: String,
) -> Result<String, String> {
    // Parse recovery key
    let key = RecoveryKey::from_base58(&recovery_key)
        .map_err(|e| format!("Invalid recovery key: {e}"))?;

    // Download from server
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .get(format!("{server_url}/api/keys/backup"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if response.status().as_u16() == 404 {
        return Err("No backup found".to_string());
    }

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    let backup_resp: BackupResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;

    // Decode base64
    let salt = STANDARD
        .decode(&backup_resp.salt)
        .map_err(|_| "Invalid salt encoding")?;
    let nonce = STANDARD
        .decode(&backup_resp.nonce)
        .map_err(|_| "Invalid nonce encoding")?;
    let ciphertext = STANDARD
        .decode(&backup_resp.ciphertext)
        .map_err(|_| "Invalid ciphertext encoding")?;

    // Reconstruct encrypted backup
    let encrypted = EncryptedBackup {
        salt: salt.try_into().map_err(|_| "Invalid salt length")?,
        nonce: nonce.try_into().map_err(|_| "Invalid nonce length")?,
        ciphertext,
        version: backup_resp.version as u32,
    };

    // Decrypt
    let decrypted = encrypted
        .decrypt(&key)
        .map_err(|e| format!("Decryption failed: {e}"))?;

    let data = String::from_utf8(decrypted)
        .map_err(|_| "Backup data is not valid UTF-8")?;

    info!("Backup restored successfully");
    Ok(data)
}
```

**Step 3: Commit**

```bash
git add client/src-tauri/src/commands/crypto.rs
git commit -m "feat(tauri): add restore_backup command"
```

---

## Task 8: Register Tauri Commands

**Files:**
- Modify: `client/src-tauri/src/main.rs`

**Step 1: Add crypto commands to invoke_handler**

```rust
// Find the invoke_handler! macro and add:
            commands::crypto::get_server_settings,
            commands::crypto::get_backup_status,
            commands::crypto::generate_recovery_key,
            commands::crypto::create_backup,
            commands::crypto::restore_backup,
```

**Step 2: Commit**

```bash
git add client/src-tauri/src/main.rs
git commit -m "feat(tauri): register crypto commands"
```

---

## Task 9: Create Recovery Key Modal Component

**Files:**
- Create: `client/src/components/settings/RecoveryKeyModal.tsx`

**Step 1: Create the modal component**

```tsx
/**
 * Recovery Key Modal
 *
 * Displays the recovery key after registration or when requested.
 * User must acknowledge saving the key before continuing.
 */

import { Component, createSignal, Show, For } from "solid-js";
import { Portal } from "solid-js/web";
import { Copy, Download, X, Shield, Check } from "lucide-solid";

interface RecoveryKeyModalProps {
  /** The recovery key chunks to display. */
  keyChunks: string[];
  /** Full key for copy/download. */
  fullKey: string;
  /** Whether this is the initial setup (shows skip option). */
  isInitialSetup?: boolean;
  /** Called when user confirms they saved the key. */
  onConfirm: () => void;
  /** Called when user skips (only if isInitialSetup). */
  onSkip?: () => void;
  /** Called to close the modal. */
  onClose: () => void;
}

const RecoveryKeyModal: Component<RecoveryKeyModalProps> = (props) => {
  const [confirmed, setConfirmed] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(props.fullKey);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownload = () => {
    const blob = new Blob([props.fullKey], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "canis-recovery-key.txt";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Portal>
      <div class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50">
        <div
          class="border border-white/10 rounded-2xl w-[500px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style="background-color: var(--color-surface-layer1)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <Shield class="w-6 h-6 text-accent-primary" />
              <h2 class="text-xl font-bold text-text-primary">
                Secure Your Messages
              </h2>
            </div>
            <Show when={!props.isInitialSetup}>
              <button
                onClick={props.onClose}
                class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
              >
                <X class="w-5 h-5" />
              </button>
            </Show>
          </div>

          {/* Content */}
          <div class="p-6 space-y-6">
            <p class="text-text-secondary">
              Your messages are end-to-end encrypted. Save your recovery key to
              restore them if you lose all devices.
            </p>

            {/* Recovery Key Display */}
            <div class="bg-surface-base rounded-xl p-4 font-mono text-lg text-center">
              <div class="grid grid-cols-4 gap-2">
                <For each={props.keyChunks}>
                  {(chunk) => (
                    <span class="text-text-primary">{chunk}</span>
                  )}
                </For>
              </div>
            </div>

            {/* Action Buttons */}
            <div class="flex gap-3">
              <button
                onClick={handleCopy}
                class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
              >
                <Show when={copied()} fallback={<Copy class="w-4 h-4" />}>
                  <Check class="w-4 h-4 text-green-400" />
                </Show>
                {copied() ? "Copied!" : "Copy"}
              </button>
              <button
                onClick={handleDownload}
                class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
              >
                <Download class="w-4 h-4" />
                Download
              </button>
            </div>

            {/* Confirmation Checkbox */}
            <label class="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={confirmed()}
                onChange={(e) => setConfirmed(e.currentTarget.checked)}
                class="w-5 h-5 rounded border-white/20 bg-surface-base text-accent-primary focus:ring-accent-primary"
              />
              <span class="text-text-secondary">
                I have saved my recovery key somewhere safe
              </span>
            </label>
          </div>

          {/* Footer */}
          <div class="flex gap-3 px-6 py-4 border-t border-white/10">
            <Show when={props.isInitialSetup && props.onSkip}>
              <button
                onClick={props.onSkip}
                class="flex-1 px-4 py-2 text-text-secondary hover:text-text-primary transition-colors"
              >
                Skip for Now
              </button>
            </Show>
            <button
              onClick={props.onConfirm}
              disabled={!confirmed()}
              class="flex-1 px-4 py-2 bg-accent-primary hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg font-medium text-white transition-colors"
            >
              Continue
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default RecoveryKeyModal;
```

**Step 2: Commit**

```bash
git add client/src/components/settings/RecoveryKeyModal.tsx
git commit -m "feat(ui): add RecoveryKeyModal component"
```

---

## Task 10: Create Security Settings Component

**Files:**
- Create: `client/src/components/settings/SecuritySettings.tsx`

**Step 1: Create the security settings component**

```tsx
/**
 * Security Settings
 *
 * Shows E2EE backup status and allows viewing recovery key.
 */

import { Component, createSignal, createResource, Show } from "solid-js";
import { Shield, AlertTriangle, Check, Eye } from "lucide-solid";

// Type for backup status
interface BackupStatus {
  has_backup: boolean;
  backup_created_at: string | null;
  version: number | null;
}

// Tauri invoke helper
async function invoke<T>(cmd: string, args?: object): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke(cmd, args);
}

interface SecuritySettingsProps {
  onViewRecoveryKey: () => void;
}

const SecuritySettings: Component<SecuritySettingsProps> = (props) => {
  const [backupStatus] = createResource<BackupStatus>(async () => {
    try {
      return await invoke<BackupStatus>("get_backup_status");
    } catch {
      return { has_backup: false, backup_created_at: null, version: null };
    }
  });

  const formatDate = (dateStr: string | null) => {
    if (!dateStr) return "Never";
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  return (
    <div class="space-y-6">
      <h3 class="text-lg font-semibold text-text-primary">Security</h3>

      {/* Backup Status Card */}
      <div class="bg-surface-base rounded-xl p-4">
        <div class="flex items-start gap-4">
          <div
            class="p-2 rounded-lg"
            classList={{
              "bg-green-500/20": backupStatus()?.has_backup,
              "bg-yellow-500/20": !backupStatus()?.has_backup,
            }}
          >
            <Show
              when={backupStatus()?.has_backup}
              fallback={<AlertTriangle class="w-6 h-6 text-yellow-400" />}
            >
              <Check class="w-6 h-6 text-green-400" />
            </Show>
          </div>

          <div class="flex-1">
            <h4 class="font-medium text-text-primary">
              {backupStatus()?.has_backup
                ? "Backup Active"
                : "Backup Not Set Up"}
            </h4>
            <p class="text-sm text-text-secondary mt-1">
              <Show
                when={backupStatus()?.has_backup}
                fallback="Your encryption keys are not backed up. If you lose all devices, you won't be able to read old messages."
              >
                Last backup: {formatDate(backupStatus()?.backup_created_at ?? null)}
              </Show>
            </p>
          </div>
        </div>

        {/* Actions */}
        <div class="mt-4 pt-4 border-t border-white/10">
          <button
            onClick={props.onViewRecoveryKey}
            class="flex items-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
          >
            <Eye class="w-4 h-4" />
            {backupStatus()?.has_backup
              ? "View Recovery Key"
              : "Set Up Backup"}
          </button>
        </div>
      </div>

      {/* Warning Banner (if no backup) */}
      <Show when={!backupStatus()?.has_backup && !backupStatus.loading}>
        <div class="flex items-center gap-3 p-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl">
          <AlertTriangle class="w-5 h-5 text-yellow-400 flex-shrink-0" />
          <p class="text-sm text-yellow-200">
            We recommend setting up a recovery key to protect your encrypted
            messages.
          </p>
        </div>
      </Show>
    </div>
  );
};

export default SecuritySettings;
```

**Step 2: Commit**

```bash
git add client/src/components/settings/SecuritySettings.tsx
git commit -m "feat(ui): add SecuritySettings component"
```

---

## Task 11: Add Security Tab to Settings Modal

**Files:**
- Modify: `client/src/components/settings/SettingsModal.tsx`
- Modify: `client/src/components/settings/index.ts`

**Step 1: Update SettingsModal.tsx**

```tsx
// Add import at top:
import { Shield } from "lucide-solid";
import SecuritySettings from "./SecuritySettings";

// Update TabId type:
type TabId = "appearance" | "audio" | "voice" | "security";

// Add security tab to tabs array:
const tabs: TabDefinition[] = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "audio", label: "Audio", icon: Volume2 },
  { id: "voice", label: "Voice", icon: Mic },
  { id: "security", label: "Security", icon: Shield },
];

// Add state for recovery key modal:
const [showRecoveryKey, setShowRecoveryKey] = createSignal(false);

// Add security tab content (inside the content area div):
              <Show when={activeTab() === "security"}>
                <SecuritySettings
                  onViewRecoveryKey={() => setShowRecoveryKey(true)}
                />
              </Show>
```

**Step 2: Update index.ts**

```typescript
export { default as SettingsModal } from "./SettingsModal";
export { default as AppearanceSettings } from "./AppearanceSettings";
export { default as SecuritySettings } from "./SecuritySettings";
export { default as RecoveryKeyModal } from "./RecoveryKeyModal";
```

**Step 3: Commit**

```bash
git add client/src/components/settings/SettingsModal.tsx client/src/components/settings/index.ts
git commit -m "feat(ui): add Security tab to SettingsModal"
```

---

## Task 12: Integrate Recovery Key Flow in Settings

**Files:**
- Modify: `client/src/components/settings/SettingsModal.tsx`

**Step 1: Add recovery key generation and modal integration**

```tsx
// Add imports at top:
import RecoveryKeyModal from "./RecoveryKeyModal";
import { createSignal, createResource } from "solid-js";

// Add state inside component:
const [showRecoveryKey, setShowRecoveryKey] = createSignal(false);
const [recoveryKey, setRecoveryKey] = createSignal<{
  fullKey: string;
  chunks: string[];
} | null>(null);
const [isGenerating, setIsGenerating] = createSignal(false);

// Add helper function:
const handleViewRecoveryKey = async () => {
  setIsGenerating(true);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const key = await invoke<{ full_key: string; chunks: string[] }>(
      "generate_recovery_key"
    );
    setRecoveryKey({ fullKey: key.full_key, chunks: key.chunks });
    setShowRecoveryKey(true);
  } catch (e) {
    console.error("Failed to generate recovery key:", e);
  } finally {
    setIsGenerating(false);
  }
};

const handleConfirmRecoveryKey = async () => {
  const key = recoveryKey();
  if (!key) return;

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // Create a simple backup with the key (actual key data would come from key store)
    const backupData = JSON.stringify({
      version: 1,
      created_at: new Date().toISOString(),
      // In real implementation, this would include actual identity keys
    });
    await invoke("create_backup", {
      recoveryKey: key.fullKey,
      backupData,
    });
    setShowRecoveryKey(false);
    setRecoveryKey(null);
  } catch (e) {
    console.error("Failed to create backup:", e);
  }
};

// Add modal at the end of the Portal content (before closing </Portal>):
      <Show when={showRecoveryKey() && recoveryKey()}>
        <RecoveryKeyModal
          keyChunks={recoveryKey()!.chunks}
          fullKey={recoveryKey()!.fullKey}
          onConfirm={handleConfirmRecoveryKey}
          onClose={() => {
            setShowRecoveryKey(false);
            setRecoveryKey(null);
          }}
        />
      </Show>

// Update SecuritySettings call:
              <Show when={activeTab() === "security"}>
                <SecuritySettings onViewRecoveryKey={handleViewRecoveryKey} />
              </Show>
```

**Step 2: Commit**

```bash
git add client/src/components/settings/SettingsModal.tsx
git commit -m "feat(ui): integrate recovery key flow in settings"
```

---

## Task 13: Add Post-Registration Prompt

**Files:**
- Modify: `client/src/App.tsx` (or main layout component)

**Step 1: Add E2EE setup prompt after registration**

This depends on your app structure. The key logic:

```tsx
// In your main app or auth flow:

const [showE2EESetup, setShowE2EESetup] = createSignal(false);
const [serverSettings, setServerSettings] = createSignal<{
  require_e2ee_setup: boolean;
} | null>(null);

// After successful registration/login:
const checkE2EESetup = async () => {
  const { invoke } = await import("@tauri-apps/api/core");

  // Get server settings
  const settings = await invoke<{ require_e2ee_setup: boolean }>(
    "get_server_settings"
  );
  setServerSettings(settings);

  // Check backup status
  const status = await invoke<{ has_backup: boolean }>("get_backup_status");

  // Show prompt if no backup and either:
  // - Server requires it
  // - This is a new registration
  if (!status.has_backup) {
    setShowE2EESetup(true);
  }
};

// If server requires E2EE and user hasn't set it up, block the app
<Show when={serverSettings()?.require_e2ee_setup && showE2EESetup()}>
  <RecoveryKeyModal
    keyChunks={recoveryKey()!.chunks}
    fullKey={recoveryKey()!.fullKey}
    isInitialSetup={true}
    onConfirm={handleConfirmRecoveryKey}
    // No skip if required
  />
</Show>

// If optional, show with skip
<Show when={!serverSettings()?.require_e2ee_setup && showE2EESetup()}>
  <RecoveryKeyModal
    keyChunks={recoveryKey()!.chunks}
    fullKey={recoveryKey()!.fullKey}
    isInitialSetup={true}
    onConfirm={handleConfirmRecoveryKey}
    onSkip={() => setShowE2EESetup(false)}
  />
</Show>
```

**Step 2: Commit**

```bash
git add client/src/App.tsx
git commit -m "feat(ui): add post-registration E2EE setup prompt"
```

---

## Task 14: Add Reminder Banner Component

**Files:**
- Create: `client/src/components/ui/BackupReminderBanner.tsx`

**Step 1: Create the banner component**

```tsx
/**
 * Backup Reminder Banner
 *
 * Shows a warning if user hasn't set up E2EE backup.
 */

import { Component } from "solid-js";
import { AlertTriangle, X } from "lucide-solid";

interface BackupReminderBannerProps {
  onSetup: () => void;
  onDismiss: () => void;
}

const BackupReminderBanner: Component<BackupReminderBannerProps> = (props) => {
  return (
    <div class="bg-yellow-500/10 border-b border-yellow-500/30 px-4 py-2 flex items-center gap-3">
      <AlertTriangle class="w-4 h-4 text-yellow-400 flex-shrink-0" />
      <p class="text-sm text-yellow-200 flex-1">
        Your encryption keys are not backed up.{" "}
        <button
          onClick={props.onSetup}
          class="underline hover:no-underline font-medium"
        >
          Set up now
        </button>
      </p>
      <button
        onClick={props.onDismiss}
        class="p-1 text-yellow-400 hover:text-yellow-200 transition-colors"
        title="Remind me later"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  );
};

export default BackupReminderBanner;
```

**Step 2: Commit**

```bash
git add client/src/components/ui/BackupReminderBanner.tsx
git commit -m "feat(ui): add BackupReminderBanner component"
```

---

## Task 15: Write Tests for Tauri Commands

**Files:**
- Modify: `client/src-tauri/src/commands/crypto.rs`

**Step 1: Add unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_key_chunks() {
        // Generate a key and verify chunking
        let key = RecoveryKey::generate();
        let full_key = key.to_base58();

        let chunks: Vec<String> = full_key
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect();

        // Each chunk should be 4 chars (except possibly the last)
        for (i, chunk) in chunks.iter().enumerate() {
            if i < chunks.len() - 1 {
                assert_eq!(chunk.len(), 4, "Chunk {} should be 4 chars", i);
            } else {
                assert!(chunk.len() <= 4, "Last chunk should be <= 4 chars");
            }
        }

        // Joining chunks should equal original
        let rejoined: String = chunks.join("");
        assert_eq!(rejoined, full_key);
    }
}
```

**Step 2: Run tests**

```bash
cd client/src-tauri && cargo test crypto
```

**Step 3: Commit**

```bash
git add client/src-tauri/src/commands/crypto.rs
git commit -m "test(tauri): add crypto command tests"
```

---

## Summary

**Server changes:**
- Added `REQUIRE_E2EE_SETUP` config option
- Added `/api/settings` endpoint
- Added `/api/keys/backup/status` endpoint

**Tauri commands:**
- `get_server_settings` - Check server config
- `get_backup_status` - Check if backup exists
- `generate_recovery_key` - Generate new key
- `create_backup` - Encrypt and upload backup
- `restore_backup` - Download and decrypt backup

**UI components:**
- `RecoveryKeyModal` - Display and save recovery key
- `SecuritySettings` - Settings tab for backup status
- `BackupReminderBanner` - Warning for missing backup

**Total: 15 tasks**
