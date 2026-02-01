# Mobile App Implementation Plan: Native Android/iOS + Shared Rust Core

## Overview

Native mobile app for the VoiceChat platform. **Android first** (Jetpack Compose + Kotlin), iOS later (SwiftUI + Swift). Shared Rust core library via **UniFFI** reusing existing `vc-crypto` crate. Includes 4 new MFA/auth mechanisms: QR Challenge-Response, QR Device Linking, FIDO2/Passkey, and Proximity-Based LAN Transfer.

---

## 1. Project Structure

```
canis-mobile/
  Cargo.toml                           # Add mobile/vc-mobile-core to workspace members
  shared/
    vc-common/                         # Existing shared types
    vc-crypto/                         # Existing E2EE crypto (vodozemac)
  mobile/
    vc-mobile-core/                    # NEW: Shared Rust core with UniFFI bindings
      Cargo.toml
      build.rs                         # uniffi-build
      src/
        lib.rs                         # UniFFI entry point
        vc_mobile_core.udl             # UniFFI interface definition
        auth.rs                        # Token storage, refresh logic
        crypto.rs                      # Wraps vc-crypto for mobile
        network/
          mod.rs
          http.rs                      # reqwest HTTP client
          websocket.rs                 # WebSocket with auto-reconnect
        store.rs                       # Encrypted SQLite key store
        qr/
          mod.rs
          challenge.rs                 # Option 1: QR Challenge-Response
          link.rs                      # Option 2: QR Device Linking
          proximity.rs                 # Option 4: Proximity Transfer
        fido2.rs                       # Option 3: FIDO2/Passkey types
        push.rs                        # Push notification token management
        voice.rs                       # WebRTC session types
        error.rs
    android/
      app/
        build.gradle.kts
        src/main/java/io/wolftown/canis/
          CanisApplication.kt
          di/                          # Hilt DI modules
          data/repository/             # AuthRepository, ChatRepository, etc.
          data/local/                  # Room DB, EncryptedSharedPreferences
          ui/auth/                     # Login, Register, MFA, QR screens
          ui/home/                     # Home, GuildList
          ui/channel/                  # TextChannel, VoiceChannel
          ui/voice/                    # VoiceCallScreen, VoiceOverlay
          ui/settings/                 # Settings, DeviceManagement, Security
          service/                     # VoiceCallService, PushService, WebSocketService
          util/                        # QR scanner/generator, BiometricHelper
      rust/                            # Generated UniFFI Kotlin bindings
    ios/                               # Phase 2 (SwiftUI, same Rust core)
  server/                              # Existing, gets new endpoints
```

### Key Dependencies: vc-mobile-core

```toml
[dependencies]
vc-common.workspace = true
vc-crypto.workspace = true
uniffi = { version = "0.28", features = ["cli"] }
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time", "macros"] }
reqwest = { version = "0.13", features = ["json", "rustls-tls"], default-features = false }
tokio-tungstenite = { version = "0.28", features = ["rustls-tls-webpki-roots"] }
x25519-dalek = "2"
ed25519-dalek = { version = "2", features = ["serde"] }
aes-gcm = "0.10"
sha2 = "0.10"
hkdf = "0.12"
vodozemac = "0.9"
rusqlite = { version = "0.32", features = ["bundled"] }
mdns-sd = "0.11"                    # mDNS for LAN discovery (Option 4)
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
rand = "0.8"
zeroize = { version = "1", features = ["derive"] }

[lib]
crate-type = ["cdylib", "staticlib"]
```

### Key Dependencies: Android

- Jetpack Compose BOM 2025.01+, Material3
- Hilt (DI), Navigation Compose
- `stream-webrtc-android` (WebRTC)
- Google ML Kit Barcode Scanning (QR scanner)
- `qrcode-kotlin` (QR generation)
- CameraX (camera access)
- AndroidX Biometric
- Firebase Messaging (FCM)
- Google Play Services FIDO2
- AndroidX Security Crypto (EncryptedSharedPreferences)
- Room (local cache)

---

## 2. UniFFI Exposed API Surface

The Rust core exposes these interfaces to Kotlin/Swift:

### Crypto
- `MobileCryptoManager` — init, identity keys, prekey generation, encrypt/decrypt messages, session management (wraps `vc-crypto` OlmAccount/OlmSession)
- `RecoveryKeyManager` — generate, create/decrypt backup

### Auth/Network
- `AuthClient` — login, register, refresh, logout, OIDC providers
- `WebSocketClient` — connect, disconnect, send events, callback interface for events

### QR/MFA (NEW)
- `QrChallengeManager` — parse login QR, sign challenge with Ed25519
- `QrDeviceLinkManager` — generate/parse device link QR, create endorsement, encrypt/decrypt key transfer
- `ProximityTransferManager` — animated QR frames, ECDH key exchange, mDNS advertise/discover, encrypt/decrypt transfer

### Async Strategy
UniFFI has limited async support. All async operations (HTTP, WebSocket, mDNS) run on a Rust-side tokio runtime. Results are delivered via UniFFI callback interfaces dispatched to Kotlin/Swift main threads. The Rust core manages its own `tokio::Runtime` initialized on first use. WebSocket events use a `WebSocketCallback` trait that Kotlin/Swift implements. This avoids blocking the JNI thread.

### Build
```bash
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o ./android/app/src/main/jniLibs build --release -p vc-mobile-core
```

---

## 3. Database Migration

**New file:** `server/migrations/20260203000000_qr_auth_and_push.sql`

### QR Login Challenges (Option 1)
```sql
CREATE TABLE qr_login_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_nonce TEXT NOT NULL UNIQUE,       -- 32 random bytes, hex
    device_fingerprint TEXT NOT NULL,         -- e.g. "Chrome/Windows"
    requesting_ip INET NOT NULL,
    requesting_user_agent TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'rejected', 'expired')),
    approved_by_user_id UUID REFERENCES users(id),
    approved_by_device_id UUID REFERENCES user_devices(id),
    signature TEXT,                           -- Ed25519 sig of nonce
    granted_session_id UUID REFERENCES sessions(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '2 minutes'
);
CREATE INDEX idx_qr_login_nonce ON qr_login_challenges(session_nonce) WHERE status = 'pending';
CREATE INDEX idx_qr_login_expires ON qr_login_challenges(expires_at);
```

### Device Endorsements (Option 2)
```sql
CREATE TABLE device_endorsements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    new_device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    endorser_device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    endorser_signature TEXT NOT NULL UNIQUE,   -- Ed25519 sig of new device pubkey (unique prevents replay)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(new_device_id, endorser_device_id)
);
```

### WebAuthn Credentials (Option 3)
```sql
CREATE TABLE webauthn_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id BYTEA NOT NULL UNIQUE,
    public_key BYTEA NOT NULL,
    sign_count BIGINT NOT NULL DEFAULT 0,
    transports TEXT[],                       -- '{usb, ble, nfc, hybrid}'
    attestation_format TEXT,
    device_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX idx_webauthn_user ON webauthn_credentials(user_id);

CREATE TABLE webauthn_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    challenge BYTEA NOT NULL UNIQUE,         -- 32 random bytes
    user_id UUID REFERENCES users(id),
    challenge_type TEXT NOT NULL CHECK (challenge_type IN ('registration', 'authentication')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '5 minutes'
);
```

### Proximity Transfers (Option 4)
```sql
CREATE TABLE proximity_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    initiator_device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    target_public_key_x25519 TEXT NOT NULL,
    relay_channel_id TEXT NOT NULL UNIQUE,    -- For server relay fallback
    encrypted_payload BYTEA,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'connected', 'completed', 'expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '5 minutes',
    CONSTRAINT payload_size CHECK (octet_length(encrypted_payload) <= 262144)  -- 256KB max
);
```

### Push Notification Tokens
```sql
CREATE TABLE push_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID REFERENCES user_devices(id) ON DELETE SET NULL,
    platform TEXT NOT NULL CHECK (platform IN ('android', 'ios', 'web')),
    token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, token)
);
CREATE INDEX idx_push_tokens_user ON push_tokens(user_id);

-- Extend user_devices with device type
ALTER TABLE user_devices ADD COLUMN device_type TEXT DEFAULT 'desktop'
    CHECK (device_type IN ('desktop', 'android', 'ios', 'web'));
```

---

## 4. Server API — New Endpoints

### Option 1: QR Challenge-Response
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/auth/qr/challenge` | No | Desktop creates QR challenge. Returns `{nonce, poll_token, qr_data, expires_at}`. `poll_token` is 32 random bytes stored in Redis key `qr:poll:{sha256(poll_token)}` with 2-min TTL. |
| GET | `/api/auth/qr/challenge/{nonce}` | Poll token | Desktop polls for approval. Requires `Authorization: Bearer <poll_token>`. Rate: 1 req/2s max. |
| POST | `/api/auth/qr/approve` | Yes | Mobile approves with device signature |

**New file:** `server/src/auth/qr_challenge.rs`

### Option 2: QR Device Linking
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/auth/devices/link/init` | No | New device initiates linking. Nonce stored in Redis with 5-min TTL (`device_link:{nonce}`). |
| POST | `/api/auth/devices/link/endorse` | Yes | Existing device endorses. Server validates: nonce exists in Redis, not expired, device_type matches QR claim. |
| POST | `/api/auth/devices/link/transfer` | Yes | Existing device sends encrypted keys |
| GET | `/api/auth/devices/link/transfer/{device_id}` | Yes | New device retrieves keys |

**New file:** `server/src/auth/device_link.rs`

### Option 3: FIDO2/Passkey
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/auth/webauthn/register/begin` | Yes | Start passkey registration |
| POST | `/api/auth/webauthn/register/complete` | Yes | Complete registration |
| POST | `/api/auth/webauthn/login/begin` | No | Start passkey login |
| POST | `/api/auth/webauthn/login/complete` | No | Complete login |
| GET | `/api/auth/webauthn/credentials` | Yes | List user's passkeys |
| DELETE | `/api/auth/webauthn/credentials/{id}` | Yes | Remove passkey |

**New file:** `server/src/auth/webauthn.rs`
**New dependency:** `webauthn-rs = "0.5"`

### Option 4: Proximity Transfer
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/auth/proximity/init` | No | New device creates relay channel. Rate: 3 per IP per hour. Relay ID stored in Redis with 5-min TTL for auto-cleanup. |
| POST | `/api/auth/proximity/relay/{id}` | Yes | Existing device uploads encrypted payload |
| GET | `/api/auth/proximity/relay/{id}` | Relay token | New device retrieves payload. Requires short-lived `relay_token` (returned from init, 32 random bytes) to prevent QR-intercept attacks. |

**New file:** `server/src/auth/proximity.rs`

### Push Notifications
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/push/register` | Yes | Register FCM/APNs token |
| POST | `/api/push/refresh` | Yes | Update token after Google/Apple refresh |
| DELETE | `/api/push/register` | Yes | Unregister token |

**New module:** `server/src/push/` (mod.rs, handlers.rs, fcm.rs, apns.rs, dispatcher.rs)

### OIDC for Mobile
Mobile OIDC uses Custom URI Scheme (`canis://oidc/callback`) registered as Android App Link / iOS Universal Link. The server's existing OIDC flow already supports custom `redirect_uri` — mobile passes its scheme-based URI. No new server endpoints needed, just client-side handling in `AuthRepository`.

---

## 5. Cryptographic Protocols

### Option 1: QR Challenge-Response

```
Desktop                    Server                     Mobile (authenticated)
   |                         |                              |
   |-- POST /qr/challenge -->|                              |
   |<-- {nonce, url, ts} ----|                              |
   | [Display QR]            |                              |
   |                         |                              | [Scan QR]
   |                         |                              | [Show: "Sign in Chrome/Win?"]
   |                         |<-- POST /qr/approve ---------|
   |                         |    {nonce, sig, device_id}    |
   |                         | [Verify Ed25519 sig of nonce  |
   |                         |  against device identity key] |
   |                         | [Create session for desktop]  |
   | [Poll: GET /qr/{nonce}] |                              |
   |<-- {approved, tokens} --|                              |
```

- **Signature:** Ed25519 sign `session_nonce` bytes using device's `identity_key_ed25519` private key
- **Nonce:** 32 random bytes, hex-encoded (64 chars)
- **TTL:** 2 minutes, one-use
- **Rate limit:** 5 per IP per minute
- **Replay protection:** Nonce consumed atomically on approve (`UPDATE ... SET status = 'approved' WHERE status = 'pending'` returning rows affected). Also cache used nonces in Redis with 2-min TTL as secondary check.
- **Poll auth:** POST `/qr/challenge` returns a `poll_token` (random, short-lived). GET `/qr/challenge/{nonce}` requires `Authorization: Bearer <poll_token>` to prevent nonce enumeration.

### Option 2: QR Device Linking

```
New Device                 Server                  Existing Device
   | [Generate X25519 pair]  |                           |
   | [Display QR: pubkey +   |                           |
   |  nonce + device_type]   |                           |
   |                         |                           | [Scan QR]
   |                         |                           | [Show: "Link iPhone 15?"]
   |                         |<-- POST /link/endorse ----|
   |                         |    {new_pubkey, sig,      |
   |                         |     endorser_device_id}   |
   |                         | [Register new device,     |
   |                         |  issue JWT]               |
   |                         |<-- POST /link/transfer ---|
   |                         |    {encrypted_olm_data}   |
   | [Poll GET /link/transfer]                           |
   |<-- {encrypted_keys} ----|                           |
   | [X25519 ECDH decrypt,   |                           |
   |  import Olm account]    |                           |
```

- **Endorsement sig:** `Ed25519_sign(SHA256(new_pubkey_bytes || nonce))` with endorser's identity key
- **TOCTOU protection:** Endorsement verification + device registration wrapped in single DB transaction with `SELECT ... FOR UPDATE` lock on endorser device row. Endorser must still exist and belong to the same user.
- **Key transfer encryption:**
  1. `shared = X25519(existing_private, new_public)`
  2. `key = HKDF-SHA256(shared, salt="vc-device-link-v1", info="key-transfer")`
  3. `ciphertext = AES-256-GCM(key, random_nonce, serialized_olm_account)`

### Option 3: FIDO2/Passkey

- Uses `webauthn-rs` crate server-side, Google Play Services FIDO2 API on Android
- RP ID = server domain
- Credential type: ES256 or Ed25519
- **`userVerification: "required"`** in both registration and authentication options (enforces biometric/PIN, not possession-only)
- **Attestation:** `attestation: "direct"`. Accept formats: `packed`, `tpm`, `android-key`, `android-safetynet`. Reject `none` in production to ensure hardware-backed authenticators.
- For cross-device auth: FIDO2 hybrid/caBLE transport (QR + BLE proximity)
- `sign_count` tracked per credential for clone detection

### Option 4: Proximity-Based LAN Transfer

```
New Device                              Existing Device
   | [Display animated QR:              |
   |  X25519 pubkey + session_id        |
   |  + mDNS service + relay_id]        |
   |                                    | [Scan animated QR]
   |                                    | [Generate own X25519 pair]
   |                                    | [ECDH shared secret]
   |                                    |
   |     [--- LAN Path (primary) ---]   |
   |                                    | [Advertise mDNS: _vc-transfer._tcp]
   | [Discover mDNS service]            |
   | [TCP+TLS(PSK) connect] ----------->|
   |<------- AES-256-GCM(shared_key, ---|
   |         serialized_keys + creds)   |
   |                                    |
   |     [--- Relay Path (fallback, 10s timeout) ---]
   |                                    |
   | POST /proximity/init              |
   |                                    | POST /proximity/relay/{id}
   | GET /proximity/relay/{id}          |
```

- **mDNS service:** `_vc-transfer._tcp`, TXT: `session=<uuid>`, `pubkey=<base64>`
- **LAN protocol:** TCP + TLS 1.3 with mutual PSK auth. PSK derived as `HKDF-SHA256(shared_secret, salt="vc-proximity-tls-v1", info=session_id)`. Both sides verify PSK identity to prevent MITM.
- **KDF:** `HKDF-SHA256(X25519_shared, salt="vc-proximity-v1", info="transfer")`
- **Animated QR:** 5 FPS cycle, each frame `{"v":1,"t":N,"i":idx,"d":"base64chunk"}`. Frames include CRC32 checksum. Scanner cycles through until all frames collected. Redundancy: each frame repeated 3x in the cycle.
- **Fallback:** Server relay after 10s if mDNS discovery fails
- **Battery:** mDNS advertisement limited to 30s max, then auto-teardown. Wi-Fi lock only during active transfer.

---

## 6. QR Code Data Formats

### Option 1: Login Challenge
```
canis://qr/login?data=<base64url>
```
Decoded: `{"t":"login","n":"<64-hex-nonce>","u":"https://chat.example.com","ts":1706745600,"fp":"Chrome/Windows"}`

### Option 2: Device Link
```
canis://qr/link?data=<base64url>
```
Decoded: `{"t":"link","pk":"<base64-X25519-pubkey>","n":"<nonce>","dt":"android"}`

### Option 3: FIDO2 Hybrid
Standard CTAP 2.2 caBLE QR format (generated by FIDO2 library).

### Option 4: Proximity (Animated)
Per-frame: `{"v":1,"t":3,"i":0,"d":"<base64-chunk>"}`
Assembled: `{"t":"proximity","pk":"<base64-X25519>","sid":"<uuid>","mdns":"_vc-transfer._tcp","relay":"<uuid>"}`

---

## 7. Push Notification Architecture

### Server Side
New module `server/src/push/` hooks into existing WebSocket broadcast system:
1. When broadcasting to a user, check if they have an active WebSocket
2. If not, query `push_tokens` table for their registered devices
3. Send via FCM v1 API (Android) or APNs (iOS, Phase 2)

### Push Notification Types
- `NewMessage` — text message in channel/DM
- `VoiceCallIncoming` — voice call ring (high priority, full-screen intent)
- `FriendRequest` — social notification
- `MentionInChannel` — @mention in channel

### Android Notification Channels
- `messages` — default importance
- `voice_calls` — high importance, full-screen intent
- `mentions` — high importance
- `friend_requests` — default importance

---

## 8. WebRTC on Mobile

- Uses `stream-webrtc-android` (Google WebRTC for Android)
- Same signaling protocol as desktop (WebSocket: VoiceJoin -> VoiceOffer -> VoiceAnswer -> ICE)
- **No server changes needed** — existing SFU works identically
- Foreground service (`VoiceCallService`) for background audio with `FOREGROUND_SERVICE_TYPE_MICROPHONE`
- Bluetooth routing via `AudioDeviceModule` + `BluetoothManager`
- Built-in Opus codec, echo cancellation, noise suppression, VAD

---

## 9. Security Considerations

### Rate Limiting (new categories)
- `QrAuth`: 5 per IP per minute
- `DeviceLink`: 3 per user per 10 minutes
- `WebAuthn`: 10 per user per minute
- `ProximityInit`: 3 per IP per 5 minutes
- `PushRegister`: 5 per user per hour

### Token Storage on Android
- Access token: EncryptedSharedPreferences (AES-256-GCM + AndroidKeyStore). Loaded into memory on app resume, refreshed if expired. Survives process death during multitasking.
- Refresh token: EncryptedSharedPreferences (AES-256-GCM + AndroidKeyStore)
- Olm account: encrypted SQLite (same pattern as desktop `LocalKeyStore`)

### Timeouts
| Operation | TTL | Cleanup |
|-----------|-----|---------|
| QR Login Challenge | 2 min | Server cron |
| Device Link | 5 min | Existing TTL mechanism |
| FIDO2 Challenge | 5 min | Server cron |
| Proximity Transfer | 5 min | Server cron |
| Push Token | 30 days inactive | Re-register on app open |

### App Security
- Certificate pinning for production
- `android:allowBackup="false"`, `android:usesCleartextTraffic="false"`
- Biometric gate for device linking and key export
- ProGuard/R8 obfuscation

### Android Permissions (AndroidManifest.xml)
- `INTERNET`, `ACCESS_NETWORK_STATE` — networking
- `CAMERA` — QR scanning
- `RECORD_AUDIO` — voice chat
- `BLUETOOTH`, `BLUETOOTH_CONNECT` — Bluetooth audio routing
- `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MICROPHONE` — voice call service
- `CHANGE_WIFI_MULTICAST_LOCK` — mDNS for proximity transfer
- `POST_NOTIFICATIONS` (Android 13+) — push notifications
- `USE_BIOMETRIC` — biometric auth gate

### UniFFI Callback Thread Safety
All UniFFI callbacks from Rust MUST be dispatched to the platform main thread:
- **Android:** Kotlin-side callback trait implementations use `Handler(Looper.getMainLooper()).post { ... }` to dispatch to main looper. Handle Activity destruction gracefully (weak reference pattern).
- **iOS (Phase 2):** Swift-side callback trait implementations use `DispatchQueue.main.async { ... }`.
- **Rust side:** The tokio runtime lives in a `OnceCell<Runtime>`, initialized on first UniFFI call. All async operations spawn tasks on this runtime.

### Push Token Lifecycle
- Server cron (hourly): delete `push_tokens` with `updated_at < NOW() - INTERVAL '30 days'`
- On FCM send failure (HTTP 404 / token invalid): delete stale token from DB immediately
- Mobile `onNewToken` callback: POST `/api/push/refresh` with old + new token for atomic swap

### SQLite Encryption
Mobile Olm key store uses the same pattern as desktop `LocalKeyStore` (`client/src-tauri/src/crypto/store.rs`): plain SQLite with AES-256-GCM application-level encryption of sensitive fields. Encryption key derived from AndroidKeyStore-backed master key via HKDF.

---

## 10. Implementation Phases

### Phase 1: Foundation
1. Create `vc-mobile-core` crate with UniFFI scaffolding
2. Wrap `vc-crypto` for mobile (crypto.rs, store.rs)
3. HTTP client + auth flow (auth.rs, network/http.rs)
4. Android project setup (Compose + Hilt + Navigation)
5. Login/Register screens
6. Basic guild/channel navigation UI

### Phase 2: Core Communication
7. WebSocket client in Rust core (network/websocket.rs)
8. Text messaging with E2EE
9. Voice chat (WebRTC integration + foreground service)
10. Push notification infrastructure (server module + Android service)
11. Friends, blocking, reporting UI

### Phase 3: MFA/Auth Features
12. Server migration (`20260203000000_qr_auth_and_push.sql`)
13. Option 1: QR Challenge-Response (server + mobile)
14. Option 2: QR Device Linking (server + mobile)
15. Option 3: FIDO2/Passkey (server + mobile)
16. Option 4: Proximity LAN Transfer (server + mobile)
17. QR scanner/generator utilities

### Phase 4: Polish + iOS
18. DM voice calls, rich presence
19. Performance optimization, battery optimization
20. iOS app (SwiftUI, same Rust core via UniFFI Swift bindings)

---

## 11. Critical Existing Files

| File | Relevance |
|------|-----------|
| `shared/vc-crypto/src/olm.rs` | Core E2EE to wrap via UniFFI |
| `shared/vc-crypto/src/recovery.rs` | Recovery key + backup to wrap |
| `client/src-tauri/src/crypto/manager.rs` | Desktop CryptoManager pattern to replicate |
| `client/src-tauri/src/crypto/store.rs` | Encrypted SQLite store pattern to replicate |
| `server/src/auth/mod.rs` | Auth router — add new QR/WebAuthn/proximity routes |
| `server/src/auth/handlers.rs` | Existing login/MFA handlers — reference for new flows |
| `server/src/auth/jwt.rs` | JWT generation — reuse for QR-approved sessions |
| `server/src/crypto/handlers.rs` | Device/prekey management — extend for device linking |
| `server/src/ws/mod.rs` | WebSocket protocol — same protocol for mobile |
| `server/src/voice/sfu.rs` | SFU — works unchanged with mobile clients |
| `server/src/ratelimit/mod.rs` | Rate limiter — add new categories |
| `server/migrations/20260119000000_e2ee_keys.sql` | Existing device schema to extend |

## 12. Verification

- **Rust core:** `cargo test -p vc-mobile-core` — unit tests for crypto, QR parsing, ECDH, mDNS
- **Android:** Compose UI tests + ViewModel unit tests via `./gradlew testDebugUnitTest`
- **Server:** Integration tests for new endpoints: `cargo test -p vc-server --test qr_auth --test device_link --test webauthn --test proximity --test push`
- **Cross-device E2E:** Manual test: display QR on desktop, scan with Android app, verify login completes
- **LAN transfer E2E:** Two devices on same network, verify mDNS discovery + encrypted key transfer
- **FIDO2 E2E:** Register passkey on mobile, use as roaming authenticator for desktop login
