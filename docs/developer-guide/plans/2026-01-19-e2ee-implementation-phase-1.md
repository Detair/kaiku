# Implementation Plan: E2EE Phase 1 (Key Infrastructure)

**Date:** 2026-01-19
**Based on:** `docs/plans/2026-01-19-e2ee-key-backup-design.md`
**Goal:** Implement the foundational key generation, storage, and backup systems required for End-to-End Encryption.

---

## 1. Shared Cryptography Library (`shared/vc-crypto`)

We need to finalize the crypto primitives before touching the client/server.

- [ ] **Update Dependencies:**
    - Add `vodozemac` (Olm implementation).
    - Add `aes-gcm` (Symmetric encryption).
    - Add `argon2` (Key derivation).
    - Add `bs58` (Recovery key encoding).
    - Add `zeroize` (Secure memory clearing).

- [ ] **Implement Key Types:**
    - `IdentityKey` (Ed25519) - Wrapper around vodozemac.
    - `CurveKey` (Curve25519) - For encryption.
    - `RecoveryKey` (32 bytes) - With `generate()` and `to_base58()` methods.

- [ ] **Implement Backup Logic:**
    - `derive_backup_key(recovery_key, salt)` -> AES key.
    - `encrypt_backup(data, recovery_key)` -> `(salt, nonce, ciphertext)`.
    - `decrypt_backup(salt, nonce, ciphertext, recovery_key)` -> `data`.

- [ ] **WASM Bindings (`shared/vc-crypto-wasm`):**
    - Expose these types and functions to the browser client.

## 2. Server-Side Infrastructure

The server acts as a dumb store for the encrypted blobs.

- [ ] **Database Schema:**
    ```sql
    CREATE TABLE user_key_backups (
        user_id UUID PRIMARY KEY REFERENCES users(id),
        salt BYTEA NOT NULL,
        nonce BYTEA NOT NULL,
        ciphertext BYTEA NOT NULL,
        version INT NOT NULL,
        created_at TIMESTAMPTZ DEFAULT NOW(),
        updated_at TIMESTAMPTZ DEFAULT NOW()
    );
    
    CREATE TABLE device_prekeys (
        user_id UUID REFERENCES users(id),
        device_id UUID NOT NULL,
        key_id INT NOT NULL,
        public_key TEXT NOT NULL,
        signature TEXT NOT NULL,
        PRIMARY KEY (user_id, device_id, key_id)
    );
    ```

- [ ] **API Handlers (`server/src/api/keys.rs`):**
    - `POST /api/keys/backup`: Upload encrypted backup.
    - `GET /api/keys/backup`: Download encrypted backup.
    - `POST /api/keys/upload`: Upload identity/prekeys (for Phase 2, but good to have struct ready).

## 3. Client-Side (Tauri/Rust)

For the native desktop app.

- [ ] **Key Store (`client/src-tauri/src/crypto/store.rs`):**
    - Implement `LocalKeyStore` struct.
    - Integration with OS Keychain (`tauri-plugin-keyring`) to store the `IdentityKey`.
    - Fallback to encrypted SQLite if keychain fails.

- [ ] **Key Generation Command:**
    - `generate_keys()`: Creates Identity/Curve keys if not present.
    - `get_recovery_key()`: Returns the Base58 recovery key for display.

- [ ] **Backup Command:**
    - `create_backup()`: Encrypts keys and uploads to server.
    - `restore_backup(recovery_key)`: Downloads and decrypts.

## 4. Client-Side (Frontend/Solid)

The UI for managing these keys.

- [ ] **Recovery Key Modal:**
    - "Setup Encryption" flow after registration.
    - Displays Base58 key in 4-character chunks.
    - "Copy" and "Download" buttons.
    - Mandatory "I have saved this" checkbox.

- [ ] **Settings UI:**
    - "Security" tab.
    - Status indicator: "Backup Active" / "Backup Missing".
    - "View Recovery Key" (requires auth).

## 5. Testing Plan

- [ ] **Unit Tests:**
    - Verify `RecoveryKey` round-trip (bytes -> base58 -> bytes).
    - Verify Backup encryption/decryption with wrong key fails.
- [ ] **Integration Tests:**
    - Upload backup -> Download backup -> Decrypt = Success.

---

## Next Steps (Phase 2)
Once keys are safe, we will implement the actual Olm session establishment and message encryption.
