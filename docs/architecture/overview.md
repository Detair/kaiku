# VoiceChat Platform - Technical Architecture

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLIENT LAYER                                    │
│                                                                              │
│   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐             │
│   │    Windows      │  │     Linux       │  │     macOS       │             │
│   │   (Tauri 2.0)   │  │   (Tauri 2.0)   │  │   (Tauri 2.0)   │             │
│   │                 │  │                 │  │                 │             │
│   │  ┌───────────┐  │  │  ┌───────────┐  │  │  ┌───────────┐  │             │
│   │  │  WebView  │  │  │  │  WebView  │  │  │  │  WebView  │  │             │
│   │  │ (Solid.js)│  │  │  │ (Solid.js)│  │  │  │ (Solid.js)│  │             │
│   │  └─────┬─────┘  │  │  └─────┬─────┘  │  │  └─────┬─────┘  │             │
│   │        │        │  │        │        │  │        │        │             │
│   │  ┌─────▼─────┐  │  │  ┌─────▼─────┐  │  │  ┌─────▼─────┐  │             │
│   │  │Rust Core  │  │  │  │Rust Core  │  │  │  │Rust Core  │  │             │
│   │  │• WebRTC   │  │  │  │• WebRTC   │  │  │  │• WebRTC   │  │             │
│   │  │• Audio    │  │  │  │• Audio    │  │  │  │• Audio    │  │             │
│   │  │• Crypto   │  │  │  │• Crypto   │  │  │  │• Crypto   │  │             │
│   │  └───────────┘  │  │  └───────────┘  │  │  └───────────┘  │             │
│   └────────┬────────┘  └────────┬────────┘  └────────┬────────┘             │
│            │                    │                    │                       │
└────────────┼────────────────────┼────────────────────┼───────────────────────┘
             │                    │                    │
             └────────────────────┼────────────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │       INTERNET            │
                    │   (TLS 1.3 encrypted)     │
                    └─────────────┬─────────────┘
                                  │
┌─────────────────────────────────┼───────────────────────────────────────────┐
│                           SERVER LAYER                                       │
│                                 │                                            │
│                    ┌────────────▼────────────┐                              │
│                    │      API Gateway        │                              │
│                    │   (Reverse Proxy)       │                              │
│                    │   • TLS Termination     │                              │
│                    │   • Rate Limiting       │                              │
│                    │   • Load Balancing      │                              │
│                    └────────────┬────────────┘                              │
│                                 │                                            │
│           ┌─────────────────────┼─────────────────────┐                     │
│           │                     │                     │                     │
│  ┌────────▼────────┐  ┌────────▼────────┐  ┌────────▼────────┐             │
│  │  Auth Service   │  │  Chat Service   │  │  Voice Service  │             │
│  │                 │  │                 │  │     (SFU)       │             │
│  │ • Local Auth    │  │ • Channels      │  │                 │             │
│  │ • OIDC/SSO      │  │ • Messages      │  │ • webrtc-rs     │             │
│  │ • MFA (TOTP)    │  │ • File Upload   │  │                 │             │
│  │ • Sessions      │  │ • E2EE (Olm/  │  │ • Opus Codec    │             │
│  │ • JWT Tokens    │  │    Megolm)    │  │ • DTLS-SRTP     │             │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘             │
│           │                    │                    │                       │
│           └─────────────────────┼─────────────────────┘                     │
│                                 │                                            │
│                    ┌────────────▼────────────┐                              │
│                    │     Data Layer          │                              │
│                    │                         │                              │
│                    │  ┌─────────────────┐    │                              │
│                    │  │   PostgreSQL    │    │                              │
│                    │  │   • Users       │    │                              │
│                    │  │   • Channels    │    │                              │
│                    │  │   • Messages    │    │                              │
│                    │  │   • Permissions │    │                              │
│                    │  └─────────────────┘    │                              │
│                    │                         │                              │
│                    │  ┌─────────────────┐    │                              │
│                    │  │     Valkey      │    │                              │
│                    │  │   • Sessions    │    │                              │
│                    │  │   • Presence    │    │                              │
│                    │  │   • Pub/Sub     │    │                              │
│                    │  └─────────────────┘    │                              │
│                    │                         │                              │
│                    │  ┌─────────────────┐    │                              │
│                    │  │   S3 Storage    │    │                              │
│                    │  │   • Files       │    │                              │
│                    │  │   • Avatars     │    │                              │
│                    │  │   • Backups     │    │                              │
│                    │  └─────────────────┘    │                              │
│                    └─────────────────────────┘                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## Component Details

### 1. Client Architecture (Tauri 2.0)

```
┌─────────────────────────────────────────────────────────────────┐
│                      TAURI CLIENT                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                    FRONTEND (WebView)                       │ │
│  │                                                             │ │
│  │  Framework: Solid.js                                        │ │
│  │  Styling:   UnoCSS (Tailwind-compatible)                    │ │
│  │  State:     Solid Stores + Signals                          │ │
│  │  Icons:     Lucide                                          │ │
│  │                                                             │ │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │ │
│  │  │   Views     │ │ Components  │ │   Stores    │           │ │
│  │  │             │ │             │ │             │           │ │
│  │  │ • Login     │ │ • Channel   │ │ • Auth      │           │ │
│  │  │ • Channels  │ │ • Message   │ │ • Channels  │           │ │
│  │  │ • Settings  │ │ • UserList  │ │ • Messages  │           │ │
│  │  │ • Voice     │ │ • VoiceBar  │ │ • Voice     │           │ │
│  │  │             │ │ • Settings  │ │ • Settings  │           │ │
│  │  └─────────────┘ └─────────────┘ └─────────────┘           │ │
│  │                                                             │ │
│  └──────────────────────────┬─────────────────────────────────┘ │
│                             │                                    │
│                      Tauri Commands                              │
│                             │                                    │
│  ┌──────────────────────────▼─────────────────────────────────┐ │
│  │                    BACKEND (Rust)                           │ │
│  │                                                             │ │
│  │  ┌─────────────────────────────────────────────────────┐   │ │
│  │  │                   Core Modules                       │   │ │
│  │  │                                                      │   │ │
│  │  │  ┌──────────────┐  ┌──────────────┐                 │   │ │
│  │  │  │    Audio     │  │   WebRTC     │                 │   │ │
│  │  │  │              │  │              │                 │   │ │
│  │  │  │ • cpal       │  │ • webrtc-rs  │                 │   │ │
│  │  │  │ • opus       │  │ • Signaling  │                 │   │ │
│  │  │  │              │  │ • DTLS-SRTP  │                 │   │ │
│  │  │  └──────────────┘  └──────────────┘                 │   │ │
│  │  │                                                      │   │ │
│  │  │  ┌──────────────┐  ┌──────────────┐                 │   │ │
│  │  │  │    Crypto    │  │   Network    │                 │   │ │
│  │  │  │              │  │              │                 │   │ │
│  │  │  │ • vodozemac  │  │ • HTTP/REST  │                 │   │ │
│  │  │  │ • Key Store  │  │ • WebSocket  │                 │   │ │
│  │  │  │ • Keyring    │  │ • rustls     │                 │   │ │
│  │  │  └──────────────┘  └──────────────┘                 │   │ │
│  │  │                                                      │   │ │
│  │  └─────────────────────────────────────────────────────┘   │ │
│  │                                                             │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Client Resource Targets

| Metric | Target | Discord for Comparison |
|--------|--------|------------------------|
| RAM (Idle) | <80 MB | ~300-400 MB |
| RAM (Voice active) | <120 MB | ~400-500 MB |
| CPU (Idle) | <1% | ~2-5% |
| CPU (Voice active) | <5% | ~5-10% |
| Binary Size | <50 MB | ~150 MB |
| Startup | <3s | ~5-10s |

---

### 2. Server Architecture

#### Auth Service

```
┌─────────────────────────────────────────────────────────────────┐
│                       AUTH SERVICE                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Endpoints:                                                      │
│  ──────────                                                      │
│  POST   /auth/register          Local user registration          │
│  POST   /auth/login             Login (local or SSO start)       │
│  POST   /auth/logout            End session                      │
│  POST   /auth/refresh           Renew access token               │
│  GET    /auth/oidc/callback     SSO callback handler             │
│  POST   /auth/mfa/setup         TOTP setup                       │
│  POST   /auth/mfa/verify        TOTP verification                │
│                                                                  │
│  Internal Functions:                                             │
│  ───────────────────                                             │
│  • Password Hashing (Argon2id)                                   │
│  • JWT Generation/Validation                                     │
│  • Session Management (Valkey)                                   │
│  • OIDC Provider Integration                                     │
│  • JIT User Provisioning                                         │
│                                                                  │
│  Token Strategy:                                                 │
│  ────────────────                                                │
│  • Access Token:  JWT, 15 min validity                           │
│  • Refresh Token: Opaque, 7 days, in Valkey                      │
│  • Session:       Valkey with user metadata                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Chat Service

```
┌─────────────────────────────────────────────────────────────────┐
│                       CHAT SERVICE                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  REST Endpoints:                                                 │
│  ───────────────                                                 │
│  GET    /channels                    List all channels           │
│  POST   /channels                    Create channel              │
│  GET    /channels/:id                Channel details             │
│  PATCH  /channels/:id                Edit channel                │
│  DELETE /channels/:id                Delete channel              │
│  GET    /channels/:id/messages       Load messages               │
│  POST   /channels/:id/messages       Send message                │
│  PATCH  /messages/:id                Edit message                │
│  DELETE /messages/:id                Delete message              │
│  POST   /upload                      Upload file                 │
│                                                                  │
│  WebSocket Events (Signaling):                                   │
│  ──────────────────────────────                                  │
│  → message.new          New message                              │
│  → message.edit         Message edited                           │
│  → message.delete       Message deleted                          │
│  → typing.start         User is typing                           │
│  → typing.stop          User stopped typing                      │
│  → presence.update      Online status changed                    │
│  → channel.update       Channel changed                          │
│                                                                  │
│  E2EE Integration:                                               │
│  ─────────────────                                               │
│  • Olm Sessions for 1:1 DMs                                      │
│  • Megolm Sessions for group channels                            │
│  • Key exchange over separate channel                            │
│  • Server stores only encrypted messages                         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Voice Service (SFU)

```
┌─────────────────────────────────────────────────────────────────┐
│                      VOICE SERVICE (SFU)                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Architecture: Selective Forwarding Unit                         │
│  ─────────────────────────────────────                           │
│                                                                  │
│     Client A          SFU Server           Client B              │
│        │                  │                   │                  │
│        │──── Offer ──────►│                   │                  │
│        │◄─── Answer ──────│                   │                  │
│        │                  │                   │                  │
│        │==== Media =======│======= Media ====►│                  │
│        │◄=== Media =======│◄====== Media =====│                  │
│        │                  │                   │                  │
│                                                                  │
│  The SFU:                                                        │
│  • Receives media from each client once                          │
│  • Forwards to all other clients                                 │
│  • No mixing/transcoding (CPU-efficient)                         │
│  • Scales better than mesh for >4 users                          │
│                                                                  │
│  WebRTC Signaling (JSON-RPC over WebSocket):                     │
│  ───────────────────────────────────────────                     │
│  → voice.join           Join voice channel                       │
│  → voice.leave          Leave voice channel                      │
│  → voice.offer          SDP Offer                                │
│  → voice.answer         SDP Answer                               │
│  → voice.ice            ICE Candidate                            │
│  → voice.mute           Self mute                                │
│  → voice.unmute         Self unmute                              │
│  ← voice.user_joined    User has joined                          │
│  ← voice.user_left      User has left                            │
│  ← voice.speaking       User is speaking                         │
│                                                                  │
│  Audio Pipeline:                                                 │
│  ──────────────                                                  │
│  Capture → Opus Encode → SRTP Encrypt → Network                  │
│  Network → SRTP Decrypt → Opus Decode → Playback                 │
│                                                                  │
│  Configurable Parameters:                                        │
│  ──────────────────────────                                      │
│  • Opus Bitrate: 24-96 kbps (default: 64 kbps)                   │
│  • Opus Frame Size: 10-60 ms (default: 20 ms)                    │
│  • Max Users per Channel: 50-100 (default: 50)                   │
│  • Jitter Buffer: 20-200 ms (adaptive)                           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

### 3. Database Schema (Overview)

```
┌─────────────────────────────────────────────────────────────────┐
│                     DATABASE SCHEMA                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐       ┌──────────────┐                        │
│  │    users     │       │   sessions   │                        │
│  ├──────────────┤       ├──────────────┤                        │
│  │ id (UUID)    │◄──────│ user_id      │                        │
│  │ username     │       │ token_hash   │                        │
│  │ display_name │       │ expires_at   │                        │
│  │ email        │       │ ip_address   │                        │
│  │ password_hash│       │ user_agent   │                        │
│  │ auth_method  │       └──────────────┘                        │
│  │ external_id  │                                                │
│  │ avatar_url   │       ┌──────────────┐                        │
│  │ status       │       │  user_keys   │                        │
│  │ mfa_secret   │       ├──────────────┤                        │
│  │ created_at   │◄──────│ user_id      │                        │
│  │ updated_at   │       │ identity_key │                        │
│  └──────┬───────┘       │ signed_prekey│                        │
│         │               │ one_time_keys│                        │
│         │               └──────────────┘                        │
│         │                                                        │
│         │               ┌──────────────┐                        │
│         │               │   channels   │                        │
│         │               ├──────────────┤                        │
│         │               │ id (UUID)    │                        │
│         │               │ name         │                        │
│         │               │ type         │◄─── voice│text│dm      │
│         │               │ category_id  │                        │
│         │               │ position     │                        │
│         │               │ topic        │                        │
│         │               │ user_limit   │                        │
│         │               │ created_at   │                        │
│         │               └──────┬───────┘                        │
│         │                      │                                 │
│         │     ┌────────────────┼────────────────┐               │
│         │     │                │                │               │
│         ▼     ▼                ▼                ▼               │
│  ┌──────────────┐       ┌──────────────┐ ┌──────────────┐       │
│  │ channel_     │       │   messages   │ │   megolm_    │       │
│  │ members      │       ├──────────────┤ │   sessions   │       │
│  ├──────────────┤       │ id (UUID)    │ ├──────────────┤       │
│  │ channel_id   │       │ channel_id   │ │ channel_id   │       │
│  │ user_id      │       │ user_id      │ │ session_id   │       │
│  │ role_id      │       │ content_enc  │◄─ encrypted    │       │
│  │ joined_at    │       │ nonce        │ │ sender_key   │       │
│  └──────────────┘       │ reply_to     │ │ created_at   │       │
│                         │ edited_at    │ └──────────────┘       │
│  ┌──────────────┐       │ created_at   │                        │
│  │    roles     │       └──────────────┘                        │
│  ├──────────────┤                                                │
│  │ id (UUID)    │       ┌──────────────┐                        │
│  │ name         │       │    files     │                        │
│  │ color        │       ├──────────────┤                        │
│  │ permissions  │◄─ JSONB│ id (UUID)    │                        │
│  │ position     │       │ message_id   │                        │
│  │ created_at   │       │ filename     │                        │
│  └──────────────┘       │ mime_type    │                        │
│                         │ size_bytes   │                        │
│                         │ s3_key       │                        │
│                         │ created_at   │                        │
│                         └──────────────┘                        │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

### 4. Encryption Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                  ENCRYPTION ARCHITECTURE                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  LAYER 1: Transport (all connections)                            │
│  ═══════════════════════════════════════                        │
│                                                                  │
│  Client ◄────── TLS 1.3 ──────► Server                          │
│                                                                  │
│  • All HTTP/WebSocket connections                                │
│  • Certificate pinning in client (optional)                      │
│  • rustls for implementation                                     │
│                                                                  │
│  ─────────────────────────────────────────────────────────────  │
│                                                                  │
│  LAYER 2: Voice (WebRTC)                                        │
│  ═══════════════════════════════════════                        │
│                                                                  │
│  MVP: DTLS-SRTP                                                  │
│  ┌─────────┐         ┌─────────┐         ┌─────────┐           │
│  │Client A │◄─DTLS──►│   SFU   │◄─DTLS──►│Client B │           │
│  └─────────┘  SRTP   └─────────┘  SRTP   └─────────┘           │
│                          │                                       │
│                    Server sees                                   │
│                    media (trusted)                               │
│                                                                  │
│  Later (Paranoid Mode): MLS                                     │
│  ┌─────────┐         ┌─────────┐         ┌─────────┐           │
│  │Client A │◄─MLS────│   SFU   │────MLS─►│Client B │           │
│  └─────────┘ E2EE    └─────────┘  E2EE   └─────────┘           │
│                          │                                       │
│                    Server sees                                   │
│                    only ciphertext                               │
│                                                                  │
│  ─────────────────────────────────────────────────────────────  │
│                                                                  │
│  LAYER 3: Text Messages                                         │
│  ═══════════════════════════════════════                        │
│                                                                  │
│  1:1 Direct Messages: Olm (Double Ratchet)                      │
│  ┌─────────┐                              ┌─────────┐           │
│  │ User A  │                              │ User B  │           │
│  │         │                              │         │           │
│  │ Olm     │◄────── Encrypted ──────────►│ Olm     │           │
│  │ Session │        Messages              │ Session │           │
│  └─────────┘                              └─────────┘           │
│       │                                        │                 │
│       └───► X3DH Key Agreement ◄───────────────┘                │
│             (One-time Prekeys)                                   │
│                                                                  │
│  Group Channels: Megolm                                         │
│  ┌─────────┐   ┌─────────┐   ┌─────────┐                       │
│  │ User A  │   │ User B  │   │ User C  │                       │
│  │         │   │         │   │         │                       │
│  │ Megolm  │   │ Megolm  │   │ Megolm  │                       │
│  │ Outbound│   │ Inbound │   │ Inbound │                       │
│  │ Session │   │ Session │   │ Session │                       │
│  └────┬────┘   └────┬────┘   └────┬────┘                       │
│       │             │             │                              │
│       │     ┌───────▼───────┐     │                              │
│       └────►│ Shared Session│◄────┘                              │
│             │ (Ratchets     │                                    │
│             │  forward only)│                                    │
│             └───────────────┘                                    │
│                                                                  │
│  Key Distribution:                                              │
│  • Olm Sessions for secure key exchange                         │
│  • Megolm Session Keys distributed via Olm                      │
│  • On User Join/Leave: Key Rotation                             │
│                                                                  │
│  ─────────────────────────────────────────────────────────────  │
│                                                                  │
│  LAYER 4: Data at Rest                                          │
│  ═══════════════════════════════════════                        │
│                                                                  │
│  • Messages: Already stored E2EE encrypted                      │
│  • Files: AES-256-GCM before S3 upload                          │
│  • Backups: Encrypted with server key                           │
│  • User Keys: In OS Keychain (client-side)                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

### 5. SSO/Identity Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    IDENTITY ARCHITECTURE                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│                     ┌─────────────────────┐                     │
│                     │    User Request     │                     │
│                     │   "Login with SSO"  │                     │
│                     └──────────┬──────────┘                     │
│                                │                                 │
│                                ▼                                 │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                     Auth Service                             ││
│  │                                                              ││
│  │  ┌─────────────────┐         ┌─────────────────────────┐   ││
│  │  │  Local Auth     │         │    OIDC Handler         │   ││
│  │  │                 │         │                         │   ││
│  │  │ • Username/Pass │         │ • Discovery             │   ││
│  │  │ • Argon2id      │         │ • Authorization URL     │   ││
│  │  │ • TOTP MFA      │         │ • Token Exchange        │   ││
│  │  └────────┬────────┘         │ • UserInfo Endpoint     │   ││
│  │           │                  └────────────┬────────────┘   ││
│  │           │                               │                 ││
│  │           │         ┌─────────────────────┘                 ││
│  │           │         │                                        ││
│  │           │         ▼                                        ││
│  │           │    ┌────────────────────────────────────────┐   ││
│  │           │    │           SSO Providers                │   ││
│  │           │    │                                        │   ││
│  │           │    │  ┌──────────┐ ┌──────────┐ ┌────────┐ │   ││
│  │           │    │  │Authentik │ │ Keycloak │ │Azure AD│ │   ││
│  │           │    │  └──────────┘ └──────────┘ └────────┘ │   ││
│  │           │    │  ┌──────────┐ ┌──────────┐ ┌────────┐ │   ││
│  │           │    │  │  Okta    │ │  Google  │ │  LDAP  │ │   ││
│  │           │    │  └──────────┘ └──────────┘ └────────┘ │   ││
│  │           │    └───────────────────┬────────────────────┘   ││
│  │           │                        │                        ││
│  │           ▼                        ▼                        ││
│  │  ┌──────────────────────────────────────────────────────┐  ││
│  │  │              Unified User Store                       │  ││
│  │  │                                                       │  ││
│  │  │  user_id:        UUID (internal)                      │  ││
│  │  │  auth_method:    local | oidc                         │  ││
│  │  │  external_id:    SSO Subject (if OIDC)               │  ││
│  │  │  provider:       authentik | keycloak | ... (if OIDC)│  ││
│  │  │  username:       Unique, for mentions                 │  ││
│  │  │  display_name:   From SSO or user-set                 │  ││
│  │  │  email:          From SSO or user-set                 │  ││
│  │  │  avatar_url:     From SSO or uploaded                 │  ││
│  │  │  roles:          Mapped from SSO groups               │  ││
│  │  │                                                       │  ││
│  │  └──────────────────────────────────────────────────────┘  ││
│  │                                                              ││
│  └──────────────────────────────────────────────────────────────┘│
│                                                                  │
│  SSO Attribute Mapping (configurable):                          │
│  ───────────────────────────────────────                        │
│  display_name:  preferred_username → name → email               │
│  avatar:        picture → avatar_url → (none)                   │
│  email:         email                                            │
│  groups:        groups → roles → (none)                         │
│                                                                  │
│  JIT Provisioning Flow:                                         │
│  ──────────────────────                                         │
│  1. User clicks "Login with SSO"                                │
│  2. Redirect to OIDC provider                                   │
│  3. User authenticates                                          │
│  4. Callback with authorization code                            │
│  5. Token exchange for ID token                                 │
│  6. Fetch UserInfo                                              │
│  7. User exists? → Create session                               │
│     User new? → Create profile, then session                    │
│  8. Redirect to app with session cookie                         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

### 6. Deployment Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                   DOCKER DEPLOYMENT                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  docker-compose.yml                                              │
│  ──────────────────                                              │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    Docker Network                            ││
│  │                   (voicechat_net)                            ││
│  │                                                              ││
│  │  ┌──────────────┐                                           ││
│  │  │   Traefik    │ ◄─── Port 443 (HTTPS)                     ││
│  │  │  (Reverse    │ ◄─── Port 80 (HTTP → HTTPS Redirect)      ││
│  │  │   Proxy)     │                                           ││
│  │  └──────┬───────┘                                           ││
│  │         │                                                    ││
│  │         ├──────────────────┬─────────────────┐              ││
│  │         │                  │                 │              ││
│  │         ▼                  ▼                 ▼              ││
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      ││
│  │  │ voicechat-   │  │ voicechat-   │  │ voicechat-   │      ││
│  │  │ api          │  │ voice        │  │ web          │      ││
│  │  │              │  │              │  │ (optional)   │      ││
│  │  │ Auth + Chat  │  │ SFU Server   │  │ Static Files │      ││
│  │  │ Services     │  │ WebRTC       │  │              │      ││
│  │  └──────┬───────┘  └──────┬───────┘  └──────────────┘      ││
│  │         │                 │                                  ││
│  │         │                 │  UDP Ports: 10000-10100          ││
│  │         │                 │  (WebRTC Media)                  ││
│  │         │                 │                                  ││
│  │         ▼                 │                                  ││
│  │  ┌──────────────┐        │                                  ││
│  │  │   Valkey     │◄───────┘                                  ││
│  │  │              │                                           ││
│  │  │ Sessions,    │                                           ││
│  │  │ Presence,    │                                           ││
│  │  │ Pub/Sub      │                                           ││
│  │  └──────────────┘                                           ││
│  │         │                                                    ││
│  │         ▼                                                    ││
│  │  ┌──────────────┐                                           ││
│  │  │  PostgreSQL  │                                           ││
│  │  │              │                                           ││
│  │  │ Persistent   │                                           ││
│  │  │ Data         │                                           ││
│  │  └──────────────┘                                           ││
│  │                                                              ││
│  └──────────────────────────────────────────────────────────────┘│
│                                                                  │
│  Volumes:                                                        │
│  ────────                                                        │
│  • postgres_data    - Database persistence                       │
│  • valkey_data      - Valkey persistence (optional)              │
│  • uploads          - Local file uploads (or S3)                 │
│  • certs            - TLS certificates (if not Let's Encrypt)    │
│                                                                  │
│  External Connections:                                          │
│  ─────────────────────                                          │
│  • S3-compatible storage (RustFS, Backblaze, AWS)               │
│  • SMTP server (for email notifications)                        │
│  • OIDC provider (Authentik, Keycloak, etc.)                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

### 7. Backup & Recovery

```
┌─────────────────────────────────────────────────────────────────┐
│                    BACKUP ARCHITECTURE                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Backup Components:                                             │
│  ───────────────────                                            │
│                                                                  │
│  1. PostgreSQL Database                                         │
│     • pg_dump daily at 03:00 UTC                                │
│     • WAL Archiving for Point-in-Time Recovery                  │
│     • Retention: 30 days                                        │
│                                                                  │
│  2. Uploaded Files                                              │
│     • S3 Sync/Versioning                                        │
│     • Or: tar + encrypt for local storage                       │
│                                                                  │
│  3. Configuration                                               │
│     • docker-compose.yml                                        │
│     • .env files (encrypted)                                    │
│     • TLS certificates                                          │
│                                                                  │
│  Backup Flow:                                                   │
│  ────────────                                                   │
│                                                                  │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐       │
│  │  Cronjob    │────►│  Backup     │────►│  S3 Bucket  │       │
│  │  (03:00)    │     │  Script     │     │             │       │
│  └─────────────┘     └─────────────┘     └─────────────┘       │
│                             │                    │               │
│                             │              Lifecycle             │
│                             │              Policy                │
│                             │                    │               │
│                             ▼                    ▼               │
│                      ┌─────────────┐     ┌─────────────┐        │
│                      │   Encrypt   │     │   Delete    │        │
│                      │  AES-256    │     │   after     │        │
│                      │             │     │   30 days   │        │
│                      └─────────────┘     └─────────────┘        │
│                                                                  │
│  Restore Process:                                               │
│  ────────────────                                               │
│                                                                  │
│  $ ./scripts/restore.sh --from s3://bucket/backup-2024-01-15    │
│                                                                  │
│  1. Stop services                                               │
│  2. Download + decrypt backup                                   │
│  3. PostgreSQL restore                                          │
│  4. Files restore                                               │
│  5. Start services                                              │
│  6. Health check                                                │
│                                                                  │
│  RTO (Recovery Time Objective): < 30 minutes                    │
│  RPO (Recovery Point Objective): < 24 hours                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Future: Kubernetes Scalability

*Status: Planning required before implementation*

For future K8s deployments requiring horizontal scaling, the current Valkey-based pub/sub architecture may need enhancement. Key considerations:

### Current Limitations for Multi-Pod Deployments
- Valkey pub/sub requires all pods to connect to the same instance
- No built-in message persistence for pod restarts
- Rate limiting state is centralized

### Potential Solutions (Requires Architecture Design)
- **NATS**: Sub-millisecond latency, Apache-2.0 licensed, excellent K8s operator support
- **Valkey Cluster**: Horizontal scaling with same API, but more operational complexity
- **Hybrid approach**: NATS for real-time pub/sub, Valkey for rate limiting and caching

### Design Principles to Preserve
- <50ms voice latency target
- Graceful degradation (fail-open for non-critical paths)
- Event sourcing patterns (call state reconstruction)

**Note**: This is documented for future planning. Current single-server and simple multi-server deployments work well with Valkey.

---

## References

- [PROJECT_SPEC.md](../project/specification.md) - Project Requirements
- [STANDARDS.md](../development/standards.md) - Standards Used
- [LICENSE_COMPLIANCE.md](../../LICENSE_COMPLIANCE.md) - License Review
