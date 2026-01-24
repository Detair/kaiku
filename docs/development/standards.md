# VoiceChat Platform - Standards & Protocols

This document lists all open standards, protocols, and libraries used. The goal is to minimize custom development and rely on proven, tested solutions.

---

## Overview: Build vs. Use

```
┌─────────────────────────────────────────────────────────────────┐
│                    BUILD vs. USE DECISION                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ✅ USE (Standards/Libraries)          ❌ DO NOT build yourself │
│  ─────────────────────────────────   ───────────────────────    │
│  • WebRTC Stack                      • Custom RTP Protocol      │
│  • Opus Codec                        • Custom Audio Codec       │
│  • TLS/DTLS                          • Custom Crypto            │
│  • Olm/Megolm (vodozemac)            • Custom E2EE Protocol     │
│  • OAuth 2.0 / OIDC                  • Custom SSO System        │
│  • Argon2 Password Hashing           • Custom Password Hash     │
│  • JWT (for API)                     • Custom Token Format      │
│  • PostgreSQL                        • Custom Database          │
│  • S3 API                            • Custom Storage Protocol  │
│  • WebSocket                         • Custom Real-Time Proto   │
│  • JSON / JSON-RPC                   • Custom Serialization     │
│  • CommonMark                        • Custom Markdown Parser   │
│                                                                  │
│  🔧 BUILD YOURSELF (Business Logic)                             │
│  ─────────────────────────────────────                          │
│  • Server Orchestration (Channels, Permissions)                 │
│  • Client UI/UX                                                  │
│  • Signaling Logic (WebRTC Coordination)                        │
│  • User/Channel Management                                       │
│  • API Endpoints                                                 │
│  • Theming System                                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 1. Communication & Real-Time

### WebRTC

| Attribute | Value |
|----------|------|
| **Standard** | W3C WebRTC + IETF RFC 8825-8835 |
| **Purpose** | Real-Time Audio/Video Communication |
| **Rust Library** | `webrtc-rs` (webrtc = "0.11") |
| **License** | MIT/Apache 2.0 |
| **Documentation** | https://webrtc.rs |

**Included Protocols:**
- ICE (Interactive Connectivity Establishment) - RFC 8445
- STUN (Session Traversal Utilities for NAT) - RFC 5389
- TURN (Traversal Using Relays around NAT) - RFC 5766
- SDP (Session Description Protocol) - RFC 4566
- RTP/RTCP (Real-time Transport Protocol) - RFC 3550
- SRTP (Secure RTP) - RFC 3711
- DTLS (Datagram TLS) - RFC 6347

### WebSocket

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 6455 |
| **Purpose** | Bidirectional Real-Time Communication |
| **Rust Library** | `tokio-tungstenite` (0.21) |
| **License** | MIT |

### JSON-RPC 2.0

| Attribute | Value |
|----------|------|
| **Standard** | jsonrpc.org Specification |
| **Purpose** | Structured Signaling Format |
| **Rust Library** | `jsonrpsee` (0.22) |
| **License** | MIT |

**Usage:** WebRTC Signaling and Real-Time Events over WebSocket.

### REST API

| Attribute | Value |
|----------|------|
| **Standard** | OpenAPI 3.1 |
| **Purpose** | API Documentation and Validation |
| **Rust Library** | `utoipa` (4.2) |
| **License** | MIT/Apache 2.0 |

---

## 2. Audio & Video Codecs

### Opus Audio Codec

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 6716 |
| **Purpose** | Voice Encoding with Low Latency |
| **Rust Library** | `opus` (0.3) / `audiopus` (0.3) |
| **License** | BSD-3 (libopus) |
| **Bitrate** | 6-510 kbit/s (default: 64 kbit/s for Voice) |
| **Frame Size** | 2.5-60 ms (default: 20 ms) |

**Why Opus:**
- Specifically optimized for voice and music
- Lowest latency of all modern codecs
- Excellent quality at low bitrates
- Open source and royalty-free

### Video Codecs (optional, for later)

| Codec | Standard | Rust Library | License | Status |
|-------|----------|--------------|--------|--------|
| VP8 | WebM Project | `vpx-rs` | BSD-3 | Recommended for compatibility |
| VP9 | WebM Project | `vpx-rs` | BSD-3 | Better quality |
| AV1 | AOMedia | `rav1e` | BSD-2 | Future, still CPU-intensive |

### Audio Processing

| Feature | Standard/Library | License |
|---------|------------------|--------|
| Echo Cancellation | WebRTC AEC (in webrtc-rs) | MIT/Apache 2.0 |
| Noise Suppression | WebRTC NS (in webrtc-rs) | MIT/Apache 2.0 |
| Noise Cancellation (AI) | `nnnoiseless` (RNNoise Port) | BSD-3 |
| Audio I/O | `cpal` | Apache 2.0 |

---

## 3. Encryption & Security

### Transport Layer Security

| Attribute | Value |
|----------|------|
| **Standard** | TLS 1.3 (IETF RFC 8446) |
| **Purpose** | Encryption of all HTTP/WebSocket connections |
| **Rust Library** | `rustls` (0.22) |
| **License** | MIT/Apache 2.0/ISC |

**Configuration:**
- Only TLS 1.3 (no older versions)
- Cipher Suites: TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256
- Certificate Pinning optional in client

### Voice Encryption (MVP)

| Attribute | Value |
|----------|------|
| **Standard** | DTLS-SRTP (IETF RFC 5764) |
| **Purpose** | Voice Stream Encryption |
| **Implementation** | Part of `webrtc-rs` |

### Voice Encryption (Paranoid Mode, later)

| Attribute | Value |
|----------|------|
| **Standard** | MLS (IETF RFC 9420) |
| **Purpose** | True End-to-End Encryption |
| **Rust Library** | `openmls` |
| **License** | MIT |

### Text E2EE

| Attribute | Value |
|----------|------|
| **Protocol** | Olm (1:1) + Megolm (Groups) |
| **Basis** | Double Ratchet Algorithm |
| **Rust Library** | `vodozemac` (0.5) |
| **License** | Apache 2.0 |
| **Developer** | Matrix.org / Element |

**Features:**
- X3DH Key Agreement
- Double Ratchet for Perfect Forward Secrecy
- Megolm for efficient group encryption
- Post-Compromise Security

**Why vodozemac instead of libsignal:**
- libsignal is AGPL 3.0 (would force project license)
- vodozemac is Apache 2.0 (compatible)
- Production-tested by Matrix/Element
- Equivalent security

### Cryptographic Primitives

| Algorithm | Standard | Rust Library | License | Usage |
|-------------|----------|--------------|--------|------------|
| AES-256-GCM | NIST FIPS 197 | `aes-gcm` | MIT/Apache 2.0 | Data at-rest |
| X25519 | IETF RFC 7748 | `x25519-dalek` | BSD-3 | Key Exchange |
| Ed25519 | IETF RFC 8032 | `ed25519-dalek` | BSD-3 | Signatures |
| SHA-256 | NIST FIPS 180-4 | `sha2` | MIT/Apache 2.0 | Hashing |
| HKDF | IETF RFC 5869 | `hkdf` | MIT/Apache 2.0 | Key Derivation |
| Argon2id | IETF RFC 9106 | `argon2` | MIT/Apache 2.0 | Password Hashing |

---

## 4. Authentication & Identity

### OpenID Connect

| Attribute | Value |
|----------|------|
| **Standard** | OpenID Connect 1.0 |
| **Basis** | OAuth 2.1 (IETF draft-ietf-oauth-v2-1) |
| **Rust Library** | `openidconnect` (3.x) |
| **License** | MIT/Apache 2.0 |

**Supported Providers:**
- Authentik
- Keycloak
- Azure AD
- Okta
- Google
- Generic OIDC Providers

### JWT (JSON Web Tokens)

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 7519 |
| **Purpose** | Access Tokens for API Access |
| **Rust Library** | `jsonwebtoken` (9.x) |
| **License** | MIT |
| **Algorithm** | EdDSA (Ed25519) or RS256 |

### TOTP (Time-based One-Time Password)

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 6238 |
| **Purpose** | Multi-Factor Authentication |
| **Rust Library** | `totp-rs` (5.x) |
| **License** | MIT |
| **Compatibility** | Google Authenticator, Authy, etc. |

### WebAuthn (later)

| Attribute | Value |
|----------|------|
| **Standard** | W3C WebAuthn Level 2 |
| **Purpose** | Hardware-based MFA (YubiKey, etc.) |
| **Rust Library** | `webauthn-rs` |
| **License** | MPL 2.0 |

---

## 5. Data Formats & Serialization

### JSON

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 8259 |
| **Purpose** | API Payloads, Configuration |
| **Rust Library** | `serde_json` (1.x) |
| **License** | MIT/Apache 2.0 |

### UUID

| Attribute | Value |
|----------|------|
| **Standard** | IETF RFC 9562 (UUIDv7) |
| **Purpose** | Unique, time-sortable IDs |
| **Rust Library** | `uuid` (1.x) |
| **License** | MIT/Apache 2.0 |

**Why UUIDv7:**
- Time-sortable (better DB performance)
- No coordination between servers needed
- Modern standard (2024)

### Timestamps

| Attribute | Value |
|----------|------|
| **Standard** | ISO 8601 |
| **Purpose** | Uniform time formats |
| **Rust Library** | `chrono` (0.4) |
| **License** | MIT/Apache 2.0 |
| **Format** | `2024-01-15T14:30:00Z` |

### CommonMark (Markdown)

| Attribute | Value |
|----------|------|
| **Standard** | commonmark.org |
| **Purpose** | Rich text in messages |
| **Rust Library** | `pulldown-cmark` (0.10) |
| **License** | MIT |

---

## 6. Data Storage

### PostgreSQL

| Attribute | Value |
|----------|------|
| **Version** | 16.x |
| **Purpose** | Persistent data storage |
| **Rust Library** | `sqlx` (0.7) |
| **License** | PostgreSQL License (MIT-like) |

**Features Used:**
- JSONB for flexible schemas (Permissions, Settings)
- UUID as Primary Keys
- Full-Text Search for message search
- Row-Level Security (optional)

### Redis

| Attribute | Value |
|----------|------|
| **Version** | 7.x |
| **Purpose** | Sessions, Caching, Pub/Sub, Presence |
| **Rust Library** | `fred` (8.x) |
| **License** | BSD-3 |

### S3-Compatible Storage

| Attribute | Value |
|----------|------|
| **Standard** | AWS S3 API (de-facto standard) |
| **Purpose** | File Storage, Backups |
| **Rust Library** | `aws-sdk-s3` (1.x) or `rust-s3` |
| **License** | Apache 2.0 |

**Compatible Backends:**
- AWS S3
- MinIO (Self-Hosted)
- Backblaze B2
- Cloudflare R2
- DigitalOcean Spaces

---

## 7. Container & Deployment

### OCI Container Standards

| Standard | Specification | Tool |
|----------|---------------|------|
| OCI Image Spec | opencontainers.org | Docker/Podman |
| OCI Runtime Spec | opencontainers.org | Docker/Podman |
| Docker Compose | docker.com | docker-compose |

### Logging

| Attribute | Value |
|----------|------|
| **Format** | JSON Lines (jsonlines.org) |
| **Rust Library** | `tracing` + `tracing-subscriber` |
| **License** | MIT |

**Log Format:**
```json
{"timestamp":"2024-01-15T14:30:00Z","level":"INFO","target":"voicechat_api","message":"User logged in","user_id":"..."}
```

### Metrics

| Attribute | Value |
|----------|------|
| **Standard** | OpenMetrics (Prometheus-compatible) |
| **Rust Library** | `metrics` + `metrics-exporter-prometheus` |
| **License** | MIT |
| **Endpoint** | `/metrics` |

---

## 8. Client & UI

### Tauri 2.0

| Attribute | Value |
|----------|------|
| **Purpose** | Cross-Platform Desktop Framework |
| **Backend** | Rust |
| **Frontend** | WebView (System WebView) |
| **License** | MIT/Apache 2.0 |
| **Documentation** | https://tauri.app |

### Frontend Framework

| Attribute | Value |
|----------|------|
| **Framework** | Solid.js |
| **License** | MIT |
| **Bundle Size** | ~7 KB |

**Why Solid.js:**
- Smallest bundle size among reactive frameworks
- Best performance (no Virtual DOM)
- TypeScript-first
- Similar API to React (easy onboarding)

### CSS

| Attribute | Value |
|----------|------|
| **Approach** | Utility-First |
| **Framework** | UnoCSS (Tailwind-compatible) |
| **License** | MIT |
| **Theming** | CSS Custom Properties |

### Accessibility

| Standard | Specification | Implementation |
|----------|---------------|-----------|
| WCAG 2.1 AA | W3C | Contrast ratios, focus indicators |
| WAI-ARIA | W3C | ARIA Attributes for Screen Reader |
| Keyboard Navigation | – | All functions accessible via keyboard |

---

## 9. Text Chat Features

### Emoji

| Attribute | Value |
|----------|------|
| **Standard** | Unicode 15.0 |
| **Rust Library** | `emojis` |
| **License** | MIT/Apache 2.0 |

### Link Previews

| Attribute | Value |
|----------|------|
| **Standard** | Open Graph Protocol (ogp.me) |
| **Fallback** | oEmbed (oembed.com) |
| **Rust Library** | Custom Implementation |

### Media Types

| Attribute | Value |
|----------|------|
| **Standard** | MIME Types (IETF RFC 6838) |
| **Rust Library** | `mime` |
| **License** | MIT/Apache 2.0 |

---

## 10. Complete Dependency List

### Server (Cargo.toml)

```toml
[dependencies]
# Web Framework - MIT
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace", "compression-gzip"] }

# Async Runtime - MIT
tokio = { version = "1", features = ["full"] }

# WebSocket - MIT
tokio-tungstenite = "0.21"

# WebRTC - MIT/Apache 2.0
webrtc = "0.11"

# Database - MIT/Apache 2.0
sqlx = { version = "0.7", features = ["postgres", "runtime-tokio", "uuid", "chrono", "json"] }

# Redis - MIT
fred = "8"

# Auth - MIT/Apache 2.0
jsonwebtoken = "9"
argon2 = "0.5"
totp-rs = "5"
openidconnect = "3"

# Crypto - MIT/Apache 2.0 + BSD-3
rustls = "0.22"
x25519-dalek = "2"
ed25519-dalek = "2"
aes-gcm = "0.10"
hkdf = "0.12"
sha2 = "0.10"

# Text E2EE - Apache 2.0
vodozemac = "0.5"

# Serialization - MIT/Apache 2.0
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Utils - MIT/Apache 2.0
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
thiserror = "1"
anyhow = "1"

# S3 - Apache 2.0
aws-sdk-s3 = "1"
aws-config = "1"

# API Documentation - MIT/Apache 2.0
utoipa = { version = "4", features = ["axum_extras"] }
utoipa-swagger-ui = { version = "6", features = ["axum"] }

# Markdown - MIT
pulldown-cmark = "0.10"

# Validation - MIT
validator = { version = "0.16", features = ["derive"] }
```

### Client Backend (Cargo.toml)

```toml
[dependencies]
# Tauri - MIT/Apache 2.0
tauri = { version = "2", features = ["protocol-asset"] }

# Async - MIT
tokio = { version = "1", features = ["full"] }

# WebRTC - MIT/Apache 2.0
webrtc = "0.11"

# Audio - Apache 2.0 + MIT
cpal = "0.15"
opus = "0.3"
nnnoiseless = "0.5"

# Crypto - Apache 2.0
vodozemac = "0.5"

# Secure Storage - MIT/Apache 2.0
keyring = "2"

# Serialization - MIT/Apache 2.0
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# HTTP Client - MIT/Apache 2.0
reqwest = { version = "0.11", features = ["json", "rustls-tls"] }
```

### Frontend (package.json)

```json
{
  "dependencies": {
    "solid-js": "^1.8.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "typescript": "^5.3.0",
    "vite": "^5.0.0",
    "vite-plugin-solid": "^2.8.0",
    "unocss": "^0.58.0",
    "lucide-solid": "^0.300.0"
  }
}
```

---

## 11. Compliance Tooling

### cargo-deny

Automatic license checking in CI/CD:

```toml
# deny.toml
[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "MPL-2.0",
    "Unicode-DFS-2016",
    "CC0-1.0",
    "Unlicense",
]

deny = [
    "GPL-2.0",
    "GPL-3.0",
    "AGPL-3.0",
    "LGPL-2.0",
    "LGPL-2.1",
    "LGPL-3.0",
]

copyleft = "deny"
unlicensed = "deny"

[[licenses.clarify]]
name = "ring"
expression = "MIT AND ISC AND OpenSSL"
license-files = [{ path = "LICENSE", hash = 0xbd0eed23 }]
```

```bash
# In CI Pipeline
cargo deny check licenses
```

---

## References

- [PROJECT_SPEC.md](../project/specification.md) - Project Requirements
- [ARCHITECTURE.md](../architecture/overview.md) - Technical Architecture
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) - Detailed License Review
