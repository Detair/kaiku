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
│  │ • OIDC/SSO      │  │ • Messages      │  │ • mediasoup/    │             │
│  │ • MFA (TOTP)    │  │ • File Upload   │  │   webrtc-rs     │             │
│  │ • Sessions      │  │ • E2EE (Olm)    │  │ • Opus Codec    │             │
│  │ • JWT Tokens    │  │ • WebSocket     │  │ • DTLS-SRTP     │             │
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
│                    │  │     Redis       │    │                              │
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
│  │  │  │ • nnnoiseless│  │ • DTLS-SRTP  │                 │   │ │
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
│  • Session Management (Redis)                                    │
│  • OIDC Provider Integration                                     │
│  • JIT User Provisioning                                         │
│                                                                  │
│  Token Strategy:                                                 │
│  ────────────────                                                │
│  • Access Token:  JWT, 15 min validity                           │
│  • Refresh Token: Opaque, 7 days, in Redis                       │
│  • Session:       Redis with user metadata                       │
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
│  │  │   Redis      │◄───────┘                                  ││
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
│  • redis_data       - Redis persistence (optional)               │
│  • uploads          - Local file uploads (or S3)                 │
│  • certs            - TLS certificates (if not Let's Encrypt)    │
│                                                                  │
│  External Connections:                                          │
│  ─────────────────────                                          │
│  • S3-compatible storage (MinIO, Backblaze, AWS)                │
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

## Technical Debt & Known Issues

*Last reviewed: 2026-01-08*

### Critical: N+1 Query in Message List Handler

**Location:** `server/src/chat/messages.rs:133-163`

The `list()` handler fetches author info individually for each message:

```rust
for msg in messages {
    let author = db::find_user_by_id(&state.db, msg.user_id).await?;
}
```

**Impact:** 100 messages = 101 database queries. This will cause performance issues at scale.

**Recommendation:** Add batch user lookup:
```rust
pub async fn find_users_by_ids(pool: &PgPool, ids: &[Uuid]) -> HashMap<Uuid, User>
```

**Priority:** Critical - fix before production

---

### Medium: Duplicated AuthorProfile Construction

**Location:** `server/src/chat/messages.rs` (lines 135-150, 212-227, 274-289)

Profile construction logic is repeated in `list`, `create`, and `update` handlers.

**Recommendation:** Add `From<User> for AuthorProfile` and `AuthorProfile::deleted(id)` factory.

**Priority:** High - reduces maintenance burden

---

### Medium: Status Serialization Fragility

**Location:** `server/src/chat/messages.rs:142`

```rust
status: format!("{:?}", u.status).to_lowercase()
```

Using Debug formatting for API output couples wire format to Rust internals.

**Recommendation:** Add explicit `UserStatus::as_str()` method.

**Priority:** Medium - breaking change if enum changes

---

### Low: Missing Channel Access Control

**Location:** `server/src/chat/messages.rs:117-126`

The `list` endpoint checks channel existence but not user membership.

**Status:** May be intentional for public community servers. Needs explicit documentation.

**Priority:** Low - document or implement based on requirements

---

### Low: No Rate Limiting

No rate limiting on API endpoints or WebSocket messages.

**Recommendation:** Add tower-governor for HTTP, per-user limits for WebSocket.

**Priority:** Medium for production - prevents abuse

---

## Persona Review Summary (2026-01-08)

### Elrond (Architecture)

**Verdict:** Fundamental approach is sound. Embedding author profiles in message responses is the right UX decision.

**Key Concerns:**
1. N+1 query pattern must be fixed before production
2. Consider introducing `UserProfileService` abstraction for presence features
3. API response shape should plan for future features (roles, badges)

### Faramir (Security)

**Verdict:** Auth middleware is correctly applied. JWT validation is sound.

**Key Concerns:**
1. Rate limiting is missing on all endpoints
2. Channel access control is not enforced
3. Add audit logging for sensitive operations
4. Consider token revocation mechanism

### Éowyn (Code Quality)

**Verdict:** Code is readable and follows project patterns.

**Key Concerns:**
1. DRY violation in AuthorProfile construction
2. Status serialization should use explicit method
3. Error types are well-structured with thiserror

### Legolas (Testing)

**Verdict:** No tests exist for message handlers.

**Key Concerns:**
1. Add integration tests for message CRUD
2. Test WebSocket broadcast on message events
3. Add performance tests for message list pagination

---

## References

- [PROJECT_SPEC.md](../project/specification.md) - Project Requirements
- [STANDARDS.md](../development/standards.md) - Standards Used
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) - License Review
