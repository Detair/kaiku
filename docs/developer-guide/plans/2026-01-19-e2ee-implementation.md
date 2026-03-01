# E2EE + Key Backup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement end-to-end encryption for DMs with key backup and recovery.

**Architecture:** Replace TODO stubs in vc-crypto with real vodozemac implementation. Add recovery key generation with Base58 encoding. Backend stores encrypted backups and manages prekeys. Client UI handles recovery key display and verification.

**Tech Stack:** vodozemac (Olm), Argon2id, AES-256-GCM, Base58, SQLx, Tauri keyring

---

## Batch 1: Core Olm Implementation

### Task 1: OlmAccount with vodozemac

**Files:**
- Modify: `shared/vc-crypto/src/olm.rs`
- Modify: `shared/vc-crypto/src/error.rs`

**Step 1: Write the failing test**

```rust
// In shared/vc-crypto/src/olm.rs at bottom
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = OlmAccount::new();
        let identity_keys = account.identity_keys();

        // Identity key should be 43 chars (base64 Ed25519)
        assert!(!identity_keys.ed25519.is_empty());
        assert!(!identity_keys.curve25519.is_empty());
    }

    #[test]
    fn test_account_generates_one_time_keys() {
        let mut account = OlmAccount::new();
        account.generate_one_time_keys(10);

        let otks = account.one_time_keys();
        assert_eq!(otks.len(), 10);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd shared/vc-crypto && cargo test test_account_creation`
Expected: FAIL - current impl has no identity_keys method

**Step 3: Write minimal implementation**

```rust
// shared/vc-crypto/src/olm.rs
use crate::{CryptoError, Result};
use serde::{Deserialize, Serialize};
use vodozemac::olm::{Account, AccountPickle, IdentityKeys, OlmMessage, Session, SessionPickle};
use vodozemac::{Curve25519PublicKey, Ed25519PublicKey, KeyId};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Identity keys for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityKeyPair {
    pub ed25519: String,
    pub curve25519: String,
}

/// User's Olm account containing identity keys.
#[derive(ZeroizeOnDrop)]
pub struct OlmAccount {
    inner: Account,
}

impl OlmAccount {
    /// Create a new Olm account with fresh keys.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Account::new(),
        }
    }

    /// Get the account's identity keys.
    pub fn identity_keys(&self) -> IdentityKeyPair {
        let keys = self.inner.identity_keys();
        IdentityKeyPair {
            ed25519: keys.ed25519.to_base64(),
            curve25519: keys.curve25519.to_base64(),
        }
    }

    /// Generate one-time prekeys.
    pub fn generate_one_time_keys(&mut self, count: usize) {
        self.inner.generate_one_time_keys(count);
    }

    /// Get unpublished one-time keys.
    pub fn one_time_keys(&self) -> Vec<(KeyId, String)> {
        self.inner
            .one_time_keys()
            .into_iter()
            .map(|(id, key)| (id, key.to_base64()))
            .collect()
    }

    /// Mark one-time keys as published (removes from unpublished set).
    pub fn mark_keys_as_published(&mut self) {
        self.inner.mark_keys_as_published();
    }

    /// Serialize the account for secure storage.
    /// Note: vodozemac uses the term "pickle" for its serialization format.
    pub fn serialize(&self, encryption_key: &[u8; 32]) -> String {
        self.inner.pickle().encrypt(encryption_key)
    }

    /// Deserialize an account from storage.
    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        let data = AccountPickle::from_encrypted(serialized, encryption_key)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
        let inner = Account::from(data);
        Ok(Self { inner })
    }

    /// Get Curve25519 public key for session creation.
    pub fn curve25519_key(&self) -> Curve25519PublicKey {
        self.inner.curve25519_key()
    }

    /// Create an outbound session to a recipient.
    pub fn create_outbound_session(
        &mut self,
        their_identity_key: &Curve25519PublicKey,
        their_one_time_key: &Curve25519PublicKey,
    ) -> OlmSession {
        let session = self.inner.create_outbound_session(
            vodozemac::olm::SessionConfig::version_2(),
            *their_identity_key,
            *their_one_time_key,
        );
        OlmSession { inner: session }
    }

    /// Create an inbound session from a prekey message.
    pub fn create_inbound_session(
        &mut self,
        their_identity_key: &Curve25519PublicKey,
        message: &vodozemac::olm::PreKeyMessage,
    ) -> Result<OlmSession> {
        let result = self
            .inner
            .create_inbound_session(*their_identity_key, message)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
        Ok(OlmSession { inner: result.session })
    }
}

impl Default for OlmAccount {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cd shared/vc-crypto && cargo test test_account`
Expected: PASS

**Step 5: Commit**

```bash
git add shared/vc-crypto/src/olm.rs
git commit -m "feat(crypto): implement OlmAccount with vodozemac"
```

---

### Task 2: OlmSession encrypt/decrypt

**Files:**
- Modify: `shared/vc-crypto/src/olm.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_session_encrypt_decrypt() {
    // Create Alice and Bob accounts
    let mut alice = OlmAccount::new();
    let mut bob = OlmAccount::new();

    // Bob generates one-time keys
    bob.generate_one_time_keys(1);
    let bob_otk = bob.one_time_keys().pop().unwrap().1;
    let bob_otk_key = Curve25519PublicKey::from_base64(&bob_otk).unwrap();

    // Alice creates outbound session to Bob
    let mut alice_session = alice.create_outbound_session(
        &bob.curve25519_key(),
        &bob_otk_key,
    );

    // Alice encrypts a message
    let plaintext = "Hello, Bob!";
    let ciphertext = alice_session.encrypt(plaintext);

    // Bob creates inbound session and decrypts
    let message = ciphertext.into_prekey_message().unwrap();
    let mut bob_session = bob.create_inbound_session(
        &alice.curve25519_key(),
        &message,
    ).unwrap();

    let decrypted = bob_session.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}
```

**Step 2: Run test to verify it fails**

Run: `cd shared/vc-crypto && cargo test test_session_encrypt_decrypt`
Expected: FAIL - encrypt returns wrong type

**Step 3: Write minimal implementation**

```rust
/// Encrypted message from Olm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    /// Message type: 0 = prekey, 1 = normal
    pub message_type: u8,
    /// Base64-encoded ciphertext
    pub ciphertext: String,
}

impl EncryptedMessage {
    /// Convert to OlmMessage for decryption
    pub fn to_olm_message(&self) -> Result<OlmMessage> {
        match self.message_type {
            0 => {
                let msg = vodozemac::olm::PreKeyMessage::from_base64(&self.ciphertext)
                    .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
                Ok(OlmMessage::PreKey(msg))
            }
            1 => {
                let msg = vodozemac::olm::Message::from_base64(&self.ciphertext)
                    .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
                Ok(OlmMessage::Normal(msg))
            }
            _ => Err(CryptoError::InvalidKey("Unknown message type".into())),
        }
    }

    /// Try to get as prekey message
    pub fn into_prekey_message(&self) -> Option<vodozemac::olm::PreKeyMessage> {
        if self.message_type == 0 {
            vodozemac::olm::PreKeyMessage::from_base64(&self.ciphertext).ok()
        } else {
            None
        }
    }
}

/// An Olm session for encrypted 1:1 communication.
pub struct OlmSession {
    inner: Session,
}

impl OlmSession {
    /// Encrypt a message.
    pub fn encrypt(&mut self, plaintext: &str) -> EncryptedMessage {
        let message = self.inner.encrypt(plaintext);
        match message {
            OlmMessage::PreKey(m) => EncryptedMessage {
                message_type: 0,
                ciphertext: m.to_base64(),
            },
            OlmMessage::Normal(m) => EncryptedMessage {
                message_type: 1,
                ciphertext: m.to_base64(),
            },
        }
    }

    /// Decrypt a message.
    pub fn decrypt(&mut self, message: &EncryptedMessage) -> Result<String> {
        let olm_message = message.to_olm_message()?;
        let plaintext = self
            .inner
            .decrypt(&olm_message)
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;
        Ok(plaintext)
    }

    /// Get session ID for storage key.
    pub fn session_id(&self) -> String {
        self.inner.session_id()
    }

    /// Serialize the session for secure storage.
    pub fn serialize(&self, encryption_key: &[u8; 32]) -> String {
        self.inner.pickle().encrypt(encryption_key)
    }

    /// Deserialize a session from storage.
    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        let data = SessionPickle::from_encrypted(serialized, encryption_key)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
        let inner = Session::from(data);
        Ok(Self { inner })
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cd shared/vc-crypto && cargo test test_session`
Expected: PASS

**Step 5: Commit**

```bash
git add shared/vc-crypto/src/olm.rs
git commit -m "feat(crypto): implement OlmSession encrypt/decrypt"
```

---

### Task 3: Recovery Key Generation

**Files:**
- Create: `shared/vc-crypto/src/recovery.rs`
- Modify: `shared/vc-crypto/src/lib.rs`
- Modify: `shared/vc-crypto/Cargo.toml`

**Step 1: Add dependencies**

```toml
# Add to shared/vc-crypto/Cargo.toml [dependencies]
bs58 = "0.5"
getrandom = "0.2"
argon2 = "0.5"
aes-gcm = "0.10"
```

**Step 2: Write the failing test**

```rust
// shared/vc-crypto/src/recovery.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_key_generation() {
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // Should be 12 groups of 4 chars separated by spaces
        let groups: Vec<&str> = formatted.split_whitespace().collect();
        assert_eq!(groups.len(), 12);
        assert!(groups.iter().all(|g| g.len() == 4));
    }

    #[test]
    fn test_recovery_key_parsing() {
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        let parsed = RecoveryKey::from_formatted_string(&formatted).unwrap();
        assert_eq!(key.0, parsed.0);
    }

    #[test]
    fn test_derive_backup_key() {
        let recovery_key = RecoveryKey::generate();
        let salt = [0u8; 16];

        let backup_key = recovery_key.derive_backup_key(&salt);
        assert_eq!(backup_key.len(), 32);

        // Same inputs should produce same output
        let backup_key2 = recovery_key.derive_backup_key(&salt);
        assert_eq!(backup_key, backup_key2);
    }
}
```

**Step 3: Write implementation**

```rust
// shared/vc-crypto/src/recovery.rs
//! Recovery Key for E2EE key backup

use crate::{CryptoError, Result};
use argon2::{Argon2, Params};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Recovery key for backing up E2EE identity keys.
/// 256-bit random value, displayed as Base58 for user storage.
#[derive(Clone, ZeroizeOnDrop)]
pub struct RecoveryKey(pub(crate) [u8; 32]);

impl RecoveryKey {
    /// Generate a new random recovery key.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        Self(bytes)
    }

    /// Format for user display: 12 groups of 4 Base58 characters.
    pub fn to_formatted_string(&self) -> String {
        let encoded = bs58::encode(&self.0).into_string();
        encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parse from formatted string (spaces ignored).
    pub fn from_formatted_string(s: &str) -> Result<Self> {
        let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = bs58::decode(&cleaned)
            .into_vec()
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid recovery key: {}", e)))?;

        if bytes.len() != 32 {
            return Err(CryptoError::InvalidKey(format!(
                "Recovery key must be 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Derive backup encryption key using Argon2id.
    pub fn derive_backup_key(&self, salt: &[u8; 16]) -> [u8; 32] {
        let params = Params::new(65536, 3, 1, Some(32)).expect("Invalid Argon2 params");
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

        let mut output = [0u8; 32];
        argon2
            .hash_password_into(&self.0, salt, &mut output)
            .expect("Argon2 hashing failed");
        output
    }
}
```

**Step 4: Add to lib.rs**

```rust
// Add to shared/vc-crypto/src/lib.rs
pub mod recovery;
pub use recovery::RecoveryKey;
```

**Step 5: Run test to verify it passes**

Run: `cd shared/vc-crypto && cargo test recovery`
Expected: PASS

**Step 6: Commit**

```bash
git add shared/vc-crypto/
git commit -m "feat(crypto): add recovery key generation with Base58 encoding"
```

---

### Task 4: Encrypted Backup Struct

**Files:**
- Modify: `shared/vc-crypto/src/recovery.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_backup_encrypt_decrypt() {
    let recovery_key = RecoveryKey::generate();
    let original_data = b"secret identity keys";

    let backup = EncryptedBackup::create(&recovery_key, original_data);

    let decrypted = backup.decrypt(&recovery_key).unwrap();
    assert_eq!(decrypted, original_data);
}

#[test]
fn test_backup_wrong_key_fails() {
    let recovery_key = RecoveryKey::generate();
    let wrong_key = RecoveryKey::generate();
    let data = b"secret";

    let backup = EncryptedBackup::create(&recovery_key, data);
    let result = backup.decrypt(&wrong_key);

    assert!(result.is_err());
}

#[test]
fn test_backup_serialization() {
    let recovery_key = RecoveryKey::generate();
    let data = b"secret";

    let backup = EncryptedBackup::create(&recovery_key, data);
    let json = serde_json::to_string(&backup).unwrap();
    let restored: EncryptedBackup = serde_json::from_str(&json).unwrap();

    let decrypted = restored.decrypt(&recovery_key).unwrap();
    assert_eq!(decrypted, data);
}
```

**Step 2: Write implementation**

```rust
// Add to shared/vc-crypto/src/recovery.rs
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};

/// Magic bytes for backup verification
const BACKUP_MAGIC: &[u8] = b"CANIS_KEYS_V1";

/// Encrypted backup of identity keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBackup {
    /// Random salt for key derivation
    pub salt: [u8; 16],
    /// AES-GCM nonce
    pub nonce: [u8; 12],
    /// Encrypted data (includes magic prefix)
    pub ciphertext: Vec<u8>,
    /// Backup version for future compatibility
    pub version: u32,
}

impl EncryptedBackup {
    /// Create an encrypted backup.
    pub fn create(recovery_key: &RecoveryKey, data: &[u8]) -> Self {
        let mut salt = [0u8; 16];
        getrandom::getrandom(&mut salt).expect("Failed to generate salt");

        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).expect("Failed to generate nonce");

        let backup_key = recovery_key.derive_backup_key(&salt);
        let cipher = Aes256Gcm::new_from_slice(&backup_key).expect("Invalid key length");
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Prepend magic bytes for verification
        let mut plaintext = BACKUP_MAGIC.to_vec();
        plaintext.extend_from_slice(data);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_slice())
            .expect("Encryption failed");

        Self {
            salt,
            nonce: nonce_bytes,
            ciphertext,
            version: 1,
        }
    }

    /// Decrypt the backup.
    pub fn decrypt(&self, recovery_key: &RecoveryKey) -> Result<Vec<u8>> {
        let backup_key = recovery_key.derive_backup_key(&self.salt);
        let cipher = Aes256Gcm::new_from_slice(&backup_key)
            .map_err(|_| CryptoError::InvalidKey("Invalid backup key".into()))?;
        let nonce = Nonce::from_slice(&self.nonce);

        let plaintext = cipher
            .decrypt(nonce, self.ciphertext.as_slice())
            .map_err(|_| CryptoError::DecryptionFailed("Backup decryption failed - wrong recovery key?".into()))?;

        // Verify magic bytes
        if plaintext.len() < BACKUP_MAGIC.len() || &plaintext[..BACKUP_MAGIC.len()] != BACKUP_MAGIC {
            return Err(CryptoError::DecryptionFailed("Invalid backup format".into()));
        }

        Ok(plaintext[BACKUP_MAGIC.len()..].to_vec())
    }
}
```

**Step 3: Run test to verify it passes**

Run: `cd shared/vc-crypto && cargo test backup`
Expected: PASS

**Step 4: Update lib.rs exports**

```rust
pub use recovery::{EncryptedBackup, RecoveryKey};
```

**Step 5: Commit**

```bash
git add shared/vc-crypto/
git commit -m "feat(crypto): add encrypted backup with AES-256-GCM"
```

---

## Batch 2: Backend Key APIs

### Task 5: Database Tables for Keys

**Files:**
- Create: `server/migrations/20260119000000_e2ee_keys.sql`

**Step 1: Create migration**

```sql
-- server/migrations/20260119000000_e2ee_keys.sql

-- User devices with identity keys
CREATE TABLE user_devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_name TEXT,
    identity_key_ed25519 TEXT NOT NULL,
    identity_key_curve25519 TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(user_id, identity_key_curve25519)
);

-- One-time prekeys for each device
CREATE TABLE prekeys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    key_id TEXT NOT NULL,
    public_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    claimed_by UUID REFERENCES users(id),
    UNIQUE(device_id, key_id)
);

-- Encrypted key backups
CREATE TABLE key_backups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    salt BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    version INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id)
);

-- Device transfer requests
CREATE TABLE device_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    encrypted_keys BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '5 minutes'
);

-- Indexes
CREATE INDEX idx_prekeys_device_unclaimed ON prekeys(device_id) WHERE claimed_at IS NULL;
CREATE INDEX idx_device_transfers_target ON device_transfers(target_device_id);
CREATE INDEX idx_device_transfers_expires ON device_transfers(expires_at);
```

**Step 2: Commit migration (migration will be run when database is available)**

```bash
git add server/migrations/
git commit -m "feat(db): add tables for E2EE keys, prekeys, and backups"
```

---

### Task 6: Key Upload API

**Files:**
- Create: `server/src/crypto/mod.rs`
- Create: `server/src/crypto/handlers.rs`
- Modify: `server/src/lib.rs`

**Step 1: Create crypto module structure**

```rust
// server/src/crypto/mod.rs
pub mod handlers;
```

**Step 2: Add to lib.rs**

```rust
// Add to server/src/lib.rs
pub mod crypto;
```

**Step 3: Implement upload handler**

```rust
// server/src/crypto/handlers.rs
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::Claims;
use crate::error::AppError;

#[derive(Debug, Deserialize)]
pub struct UploadKeysRequest {
    pub device_name: Option<String>,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
    pub one_time_prekeys: Vec<PrekeyUpload>,
}

#[derive(Debug, Deserialize)]
pub struct PrekeyUpload {
    pub key_id: String,
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct UploadKeysResponse {
    pub device_id: Uuid,
    pub prekeys_uploaded: usize,
}

/// Upload identity keys and prekeys for a device.
/// POST /api/keys/upload
#[tracing::instrument(skip(pool))]
pub async fn upload_keys(
    State(pool): State<PgPool>,
    claims: Claims,
    Json(req): Json<UploadKeysRequest>,
) -> Result<Json<UploadKeysResponse>, AppError> {
    let user_id = claims.user_id();

    // Insert or update device
    let device_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, identity_key_curve25519)
        DO UPDATE SET last_seen_at = NOW(), device_name = COALESCE(EXCLUDED.device_name, user_devices.device_name)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&req.device_name)
    .bind(&req.identity_key_ed25519)
    .bind(&req.identity_key_curve25519)
    .fetch_one(&pool)
    .await?;

    // Insert prekeys
    let mut prekeys_uploaded = 0;
    for prekey in &req.one_time_prekeys {
        let result = sqlx::query(
            r#"
            INSERT INTO prekeys (device_id, key_id, public_key)
            VALUES ($1, $2, $3)
            ON CONFLICT (device_id, key_id) DO NOTHING
            "#,
        )
        .bind(device_id)
        .bind(&prekey.key_id)
        .bind(&prekey.public_key)
        .execute(&pool)
        .await?;

        if result.rows_affected() > 0 {
            prekeys_uploaded += 1;
        }
    }

    Ok(Json(UploadKeysResponse {
        device_id,
        prekeys_uploaded,
    }))
}

/// Get a user's device keys for encryption.
/// GET /api/users/:id/keys
#[derive(Debug, Serialize)]
pub struct UserKeysResponse {
    pub devices: Vec<DeviceKeys>,
}

#[derive(Debug, Serialize)]
pub struct DeviceKeys {
    pub device_id: Uuid,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
}

#[tracing::instrument(skip(pool))]
pub async fn get_user_keys(
    State(pool): State<PgPool>,
    _claims: Claims,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserKeysResponse>, AppError> {
    let devices = sqlx::query_as!(
        DeviceKeys,
        r#"
        SELECT id as device_id, identity_key_ed25519, identity_key_curve25519
        FROM user_devices
        WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(UserKeysResponse { devices }))
}
```

**Step 4: Run to verify it compiles**

Run: `cd server && SQLX_OFFLINE=true cargo build`
Expected: Compiles successfully (routes will be added in Task 8)

**Step 5: Commit**

```bash
git add server/src/crypto/ server/src/lib.rs
git commit -m "feat(api): add key upload and retrieval handlers"
```

---

### Task 7: Prekey Claim API

**Files:**
- Modify: `server/src/crypto/handlers.rs`

**Step 1: Add claim handler**

```rust
// Add to server/src/crypto/handlers.rs

#[derive(Debug, Deserialize)]
pub struct ClaimPrekeyRequest {
    pub device_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ClaimPrekeyResponse {
    pub device_id: Uuid,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
    pub one_time_prekey: Option<ClaimedPrekey>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClaimedPrekey {
    pub key_id: String,
    pub public_key: String,
}

/// Claim a prekey for a specific device (atomic).
/// POST /api/users/:id/keys/claim
#[tracing::instrument(skip(pool))]
pub async fn claim_prekey(
    State(pool): State<PgPool>,
    claims: Claims,
    Path(target_user_id): Path<Uuid>,
    Json(req): Json<ClaimPrekeyRequest>,
) -> Result<Json<ClaimPrekeyResponse>, AppError> {
    let claimer_id = claims.user_id();

    // Get device info
    let device = sqlx::query!(
        r#"
        SELECT identity_key_ed25519, identity_key_curve25519
        FROM user_devices
        WHERE id = $1 AND user_id = $2
        "#,
        req.device_id,
        target_user_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Device not found".into()))?;

    // Atomically claim one prekey
    let prekey = sqlx::query_as!(
        ClaimedPrekey,
        r#"
        UPDATE prekeys
        SET claimed_at = NOW(), claimed_by = $1
        WHERE id = (
            SELECT id FROM prekeys
            WHERE device_id = $2 AND claimed_at IS NULL
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING key_id, public_key
        "#,
        claimer_id,
        req.device_id
    )
    .fetch_optional(&pool)
    .await?;

    Ok(Json(ClaimPrekeyResponse {
        device_id: req.device_id,
        identity_key_ed25519: device.identity_key_ed25519,
        identity_key_curve25519: device.identity_key_curve25519,
        one_time_prekey: prekey,
    }))
}
```

**Step 2: Commit**

```bash
git add server/src/crypto/handlers.rs
git commit -m "feat(api): add atomic prekey claim handler"
```

---

### Task 8: Backup API and Routes

**Files:**
- Modify: `server/src/crypto/handlers.rs`
- Modify: `server/src/api/mod.rs` (or equivalent router file)

**Step 1: Add backup handlers**

```rust
// Add to server/src/crypto/handlers.rs

#[derive(Debug, Deserialize)]
pub struct UploadBackupRequest {
    pub salt: String,      // Base64
    pub nonce: String,     // Base64
    pub ciphertext: String, // Base64
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct BackupResponse {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
    pub version: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Upload encrypted key backup.
/// POST /api/keys/backup
#[tracing::instrument(skip(pool, req))]
pub async fn upload_backup(
    State(pool): State<PgPool>,
    claims: Claims,
    Json(req): Json<UploadBackupRequest>,
) -> Result<StatusCode, AppError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let user_id = claims.user_id();
    let salt = STANDARD.decode(&req.salt).map_err(|_| AppError::BadRequest("Invalid salt".into()))?;
    let nonce = STANDARD.decode(&req.nonce).map_err(|_| AppError::BadRequest("Invalid nonce".into()))?;
    let ciphertext = STANDARD.decode(&req.ciphertext).map_err(|_| AppError::BadRequest("Invalid ciphertext".into()))?;

    sqlx::query(
        r#"
        INSERT INTO key_backups (user_id, salt, nonce, ciphertext, version)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id) DO UPDATE SET
            salt = EXCLUDED.salt,
            nonce = EXCLUDED.nonce,
            ciphertext = EXCLUDED.ciphertext,
            version = EXCLUDED.version,
            created_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(&salt)
    .bind(&nonce)
    .bind(&ciphertext)
    .bind(req.version)
    .execute(&pool)
    .await?;

    Ok(StatusCode::CREATED)
}

/// Download encrypted key backup.
/// GET /api/keys/backup
#[tracing::instrument(skip(pool))]
pub async fn get_backup(
    State(pool): State<PgPool>,
    claims: Claims,
) -> Result<Json<BackupResponse>, AppError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let user_id = claims.user_id();

    let backup = sqlx::query!(
        r#"
        SELECT salt, nonce, ciphertext, version, created_at
        FROM key_backups
        WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound("No backup found".into()))?;

    Ok(Json(BackupResponse {
        salt: STANDARD.encode(&backup.salt),
        nonce: STANDARD.encode(&backup.nonce),
        ciphertext: STANDARD.encode(&backup.ciphertext),
        version: backup.version,
        created_at: backup.created_at,
    }))
}
```

**Step 2: Add all routes to API router**

Find the main API router (likely in `server/src/api/mod.rs` or `server/src/main.rs`) and add:

```rust
use crate::crypto::handlers as crypto;

// In the router setup, add these routes:
.route("/api/keys/upload", post(crypto::upload_keys))
.route("/api/keys/backup", post(crypto::upload_backup).get(crypto::get_backup))
.route("/api/users/:id/keys", get(crypto::get_user_keys))
.route("/api/users/:id/keys/claim", post(crypto::claim_prekey))
```

**Step 3: Run to verify it compiles**

Run: `cd server && SQLX_OFFLINE=true cargo build`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add server/
git commit -m "feat(api): add backup endpoints and wire up all crypto routes"
```

---

## Batch 3: Client Foundation (Future Plan)

*The following tasks will be written in a follow-up implementation plan:*

- Task 9: Tauri keychain integration
- Task 10: LocalKeyStore with encrypted SQLite
- Task 11: Recovery Key UI modal
- Task 12: Device verification QR flow
- Task 13: Message encryption integration
- Task 14: WASM build for browser support

---

## Summary

**Batch 1 (Tasks 1-4):** Core Olm crypto in `vc-crypto`
- OlmAccount with vodozemac
- OlmSession encrypt/decrypt
- RecoveryKey generation (Base58)
- EncryptedBackup (AES-256-GCM)

**Batch 2 (Tasks 5-8):** Backend APIs
- Database tables for devices, prekeys, backups
- Key upload endpoint
- Atomic prekey claim
- Backup upload/download

**Estimated completion:** Batch 1-2 establish the foundation for E2EE. Client integration follows in Batch 3.
