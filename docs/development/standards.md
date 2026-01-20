# VoiceChat Platform - Standards & Protokolle

Dieses Dokument listet alle verwendeten offenen Standards, Protokolle und Libraries auf. Das Ziel ist, Eigenentwicklungen zu minimieren und auf bewährte, geprüfte Lösungen zu setzen.

---

## Übersicht: Build vs. Use

```
┌─────────────────────────────────────────────────────────────────┐
│                    BUILD vs. USE DECISION                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ✅ NUTZEN (Standards/Libraries)     ❌ NICHT selbst bauen      │
│  ─────────────────────────────────   ───────────────────────    │
│  • WebRTC Stack                      • Eigenes RTP Protocol     │
│  • Opus Codec                        • Eigener Audio Codec      │
│  • TLS/DTLS                          • Eigene Crypto            │
│  • Olm/Megolm (vodozemac)            • Eigenes E2EE Protocol    │
│  • OAuth 2.0 / OIDC                  • Eigenes SSO System       │
│  • Argon2 Password Hashing           • Eigenes Passwort-Hash    │
│  • JWT (für API)                     • Eigenes Token-Format     │
│  • PostgreSQL                        • Eigene Datenbank         │
│  • S3 API                            • Eigenes Storage Protocol │
│  • WebSocket                         • Eigenes Real-Time Proto  │
│  • JSON / JSON-RPC                   • Eigene Serialisierung    │
│  • CommonMark                        • Eigener Markdown Parser  │
│                                                                  │
│  🔧 SELBST ENTWICKELN (Business Logic)                          │
│  ─────────────────────────────────────                          │
│  • Server-Orchestrierung (Channels, Permissions)                │
│  • Client UI/UX                                                  │
│  • Signaling Logic (WebRTC Coordination)                        │
│  • User/Channel Management                                       │
│  • API Endpoints                                                 │
│  • Theming System                                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 1. Kommunikation & Real-Time

### WebRTC

| Attribut | Wert |
|----------|------|
| **Standard** | W3C WebRTC + IETF RFC 8825-8835 |
| **Zweck** | Real-Time Audio/Video Kommunikation |
| **Rust Library** | `webrtc-rs` (webrtc = "0.11") |
| **Lizenz** | MIT/Apache 2.0 |
| **Dokumentation** | https://webrtc.rs |

**Enthaltene Protokolle:**
- ICE (Interactive Connectivity Establishment) - RFC 8445
- STUN (Session Traversal Utilities for NAT) - RFC 5389
- TURN (Traversal Using Relays around NAT) - RFC 5766
- SDP (Session Description Protocol) - RFC 4566
- RTP/RTCP (Real-time Transport Protocol) - RFC 3550
- SRTP (Secure RTP) - RFC 3711
- DTLS (Datagram TLS) - RFC 6347

### WebSocket

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 6455 |
| **Zweck** | Bidirektionale Real-Time Kommunikation |
| **Rust Library** | `tokio-tungstenite` (0.21) |
| **Lizenz** | MIT |

### JSON-RPC 2.0

| Attribut | Wert |
|----------|------|
| **Standard** | jsonrpc.org Spezifikation |
| **Zweck** | Strukturiertes Signaling-Format |
| **Rust Library** | `jsonrpsee` (0.22) |
| **Lizenz** | MIT |

**Verwendung:** WebRTC Signaling und Real-Time Events über WebSocket.

### REST API

| Attribut | Wert |
|----------|------|
| **Standard** | OpenAPI 3.1 |
| **Zweck** | API-Dokumentation und -Validierung |
| **Rust Library** | `utoipa` (4.2) |
| **Lizenz** | MIT/Apache 2.0 |

---

## 2. Audio & Video Codecs

### Opus Audio Codec

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 6716 |
| **Zweck** | Voice-Encoding mit niedriger Latenz |
| **Rust Library** | `opus` (0.3) / `audiopus` (0.3) |
| **Lizenz** | BSD-3 (libopus) |
| **Bitrate** | 6-510 kbit/s (default: 64 kbit/s für Voice) |
| **Frame Size** | 2.5-60 ms (default: 20 ms) |

**Warum Opus:**
- Speziell für Voice und Musik optimiert
- Niedrigste Latenz aller modernen Codecs
- Hervorragende Qualität bei niedrigen Bitraten
- Open Source und lizenzfrei

### Video Codecs (optional, für später)

| Codec | Standard | Rust Library | Lizenz | Status |
|-------|----------|--------------|--------|--------|
| VP8 | WebM Project | `vpx-rs` | BSD-3 | Empfohlen für Kompatibilität |
| VP9 | WebM Project | `vpx-rs` | BSD-3 | Bessere Qualität |
| AV1 | AOMedia | `rav1e` | BSD-2 | Zukunft, noch CPU-intensiv |

### Audio Processing

| Feature | Standard/Library | Lizenz |
|---------|------------------|--------|
| Echo Cancellation | WebRTC AEC (in webrtc-rs) | MIT/Apache 2.0 |
| Noise Suppression | WebRTC NS (in webrtc-rs) | MIT/Apache 2.0 |
| Noise Cancellation (KI) | `nnnoiseless` (RNNoise Port) | BSD-3 |
| Audio I/O | `cpal` | Apache 2.0 |

---

## 3. Verschlüsselung & Sicherheit

### Transport Layer Security

| Attribut | Wert |
|----------|------|
| **Standard** | TLS 1.3 (IETF RFC 8446) |
| **Zweck** | Verschlüsselung aller HTTP/WebSocket Verbindungen |
| **Rust Library** | `rustls` (0.22) |
| **Lizenz** | MIT/Apache 2.0/ISC |

**Konfiguration:**
- Nur TLS 1.3 (keine älteren Versionen)
- Cipher Suites: TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256
- Certificate Pinning optional im Client

### Voice Encryption (MVP)

| Attribut | Wert |
|----------|------|
| **Standard** | DTLS-SRTP (IETF RFC 5764) |
| **Zweck** | Voice-Stream-Verschlüsselung |
| **Implementation** | Teil von `webrtc-rs` |

### Voice Encryption (Paranoid Mode, später)

| Attribut | Wert |
|----------|------|
| **Standard** | MLS (IETF RFC 9420) |
| **Zweck** | Echte Ende-zu-Ende-Verschlüsselung |
| **Rust Library** | `openmls` |
| **Lizenz** | MIT |

### Text E2EE

| Attribut | Wert |
|----------|------|
| **Protokoll** | Olm (1:1) + Megolm (Gruppen) |
| **Basis** | Double Ratchet Algorithm |
| **Rust Library** | `vodozemac` (0.5) |
| **Lizenz** | Apache 2.0 |
| **Entwickler** | Matrix.org / Element |

**Features:**
- X3DH Key Agreement
- Double Ratchet für Perfect Forward Secrecy
- Megolm für effiziente Gruppen-Verschlüsselung
- Post-Compromise Security

**Warum vodozemac statt libsignal:**
- libsignal ist AGPL 3.0 (würde Projekt-Lizenz erzwingen)
- vodozemac ist Apache 2.0 (kompatibel)
- Production-tested durch Matrix/Element
- Äquivalente Sicherheit

### Kryptografische Primitive

| Algorithmus | Standard | Rust Library | Lizenz | Verwendung |
|-------------|----------|--------------|--------|------------|
| AES-256-GCM | NIST FIPS 197 | `aes-gcm` | MIT/Apache 2.0 | Daten at-rest |
| X25519 | IETF RFC 7748 | `x25519-dalek` | BSD-3 | Key Exchange |
| Ed25519 | IETF RFC 8032 | `ed25519-dalek` | BSD-3 | Signaturen |
| SHA-256 | NIST FIPS 180-4 | `sha2` | MIT/Apache 2.0 | Hashing |
| HKDF | IETF RFC 5869 | `hkdf` | MIT/Apache 2.0 | Key Derivation |
| Argon2id | IETF RFC 9106 | `argon2` | MIT/Apache 2.0 | Password Hashing |

---

## 4. Authentifizierung & Identität

### OpenID Connect

| Attribut | Wert |
|----------|------|
| **Standard** | OpenID Connect 1.0 |
| **Basis** | OAuth 2.1 (IETF draft-ietf-oauth-v2-1) |
| **Rust Library** | `openidconnect` (3.x) |
| **Lizenz** | MIT/Apache 2.0 |

**Unterstützte Provider:**
- Authentik
- Keycloak
- Azure AD
- Okta
- Google
- Generische OIDC Provider

### JWT (JSON Web Tokens)

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 7519 |
| **Zweck** | Access Tokens für API-Zugriff |
| **Rust Library** | `jsonwebtoken` (9.x) |
| **Lizenz** | MIT |
| **Algorithmus** | EdDSA (Ed25519) oder RS256 |

### TOTP (Time-based One-Time Password)

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 6238 |
| **Zweck** | Multi-Faktor-Authentifizierung |
| **Rust Library** | `totp-rs` (5.x) |
| **Lizenz** | MIT |
| **Kompatibilität** | Google Authenticator, Authy, etc. |

### WebAuthn (später)

| Attribut | Wert |
|----------|------|
| **Standard** | W3C WebAuthn Level 2 |
| **Zweck** | Hardware-basierte MFA (YubiKey, etc.) |
| **Rust Library** | `webauthn-rs` |
| **Lizenz** | MPL 2.0 |

---

## 5. Datenformate & Serialisierung

### JSON

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 8259 |
| **Zweck** | API Payloads, Konfiguration |
| **Rust Library** | `serde_json` (1.x) |
| **Lizenz** | MIT/Apache 2.0 |

### UUID

| Attribut | Wert |
|----------|------|
| **Standard** | IETF RFC 9562 (UUIDv7) |
| **Zweck** | Eindeutige, zeitlich sortierbare IDs |
| **Rust Library** | `uuid` (1.x) |
| **Lizenz** | MIT/Apache 2.0 |

**Warum UUIDv7:**
- Zeitlich sortierbar (bessere DB-Performance)
- Keine Koordination zwischen Servern nötig
- Moderner Standard (2024)

### Timestamps

| Attribut | Wert |
|----------|------|
| **Standard** | ISO 8601 |
| **Zweck** | Einheitliche Zeitformate |
| **Rust Library** | `chrono` (0.4) |
| **Lizenz** | MIT/Apache 2.0 |
| **Format** | `2024-01-15T14:30:00Z` |

### CommonMark (Markdown)

| Attribut | Wert |
|----------|------|
| **Standard** | commonmark.org |
| **Zweck** | Rich-Text in Nachrichten |
| **Rust Library** | `pulldown-cmark` (0.10) |
| **Lizenz** | MIT |

---

## 6. Datenspeicherung

### PostgreSQL

| Attribut | Wert |
|----------|------|
| **Version** | 16.x |
| **Zweck** | Persistente Datenspeicherung |
| **Rust Library** | `sqlx` (0.7) |
| **Lizenz** | PostgreSQL License (MIT-ähnlich) |

**Features genutzt:**
- JSONB für flexible Schemas (Permissions, Settings)
- UUID als Primary Keys
- Full-Text Search für Nachrichtensuche
- Row-Level Security (optional)

### Redis

| Attribut | Wert |
|----------|------|
| **Version** | 7.x |
| **Zweck** | Sessions, Caching, Pub/Sub, Presence |
| **Rust Library** | `fred` (8.x) |
| **Lizenz** | BSD-3 |

### S3-Compatible Storage

| Attribut | Wert |
|----------|------|
| **Standard** | AWS S3 API (de-facto Standard) |
| **Zweck** | File Storage, Backups |
| **Rust Library** | `aws-sdk-s3` (1.x) oder `rust-s3` |
| **Lizenz** | Apache 2.0 |

**Kompatible Backends:**
- AWS S3
- MinIO (Self-Hosted)
- Backblaze B2
- Cloudflare R2
- DigitalOcean Spaces

---

## 7. Container & Deployment

### OCI Container Standards

| Standard | Spezifikation | Tool |
|----------|---------------|------|
| OCI Image Spec | opencontainers.org | Docker/Podman |
| OCI Runtime Spec | opencontainers.org | Docker/Podman |
| Docker Compose | docker.com | docker-compose |

### Logging

| Attribut | Wert |
|----------|------|
| **Format** | JSON Lines (jsonlines.org) |
| **Rust Library** | `tracing` + `tracing-subscriber` |
| **Lizenz** | MIT |

**Log-Format:**
```json
{"timestamp":"2024-01-15T14:30:00Z","level":"INFO","target":"voicechat_api","message":"User logged in","user_id":"..."}
```

### Metrics

| Attribut | Wert |
|----------|------|
| **Standard** | OpenMetrics (Prometheus-kompatibel) |
| **Rust Library** | `metrics` + `metrics-exporter-prometheus` |
| **Lizenz** | MIT |
| **Endpoint** | `/metrics` |

---

## 8. Client & UI

### Tauri 2.0

| Attribut | Wert |
|----------|------|
| **Zweck** | Cross-Platform Desktop Framework |
| **Backend** | Rust |
| **Frontend** | WebView (System WebView) |
| **Lizenz** | MIT/Apache 2.0 |
| **Dokumentation** | https://tauri.app |

### Frontend Framework

| Attribut | Wert |
|----------|------|
| **Framework** | Solid.js |
| **Lizenz** | MIT |
| **Bundle Size** | ~7 KB |

**Warum Solid.js:**
- Kleinster Bundle Size unter reaktiven Frameworks
- Beste Performance (kein Virtual DOM)
- TypeScript-first
- Ähnliche API wie React (leichter Einstieg)

### CSS

| Attribut | Wert |
|----------|------|
| **Approach** | Utility-First |
| **Framework** | UnoCSS (Tailwind-kompatibel) |
| **Lizenz** | MIT |
| **Theming** | CSS Custom Properties |

### Accessibility

| Standard | Spezifikation | Umsetzung |
|----------|---------------|-----------|
| WCAG 2.1 AA | W3C | Kontrastverhältnisse, Fokusindikatoren |
| WAI-ARIA | W3C | ARIA Attributes für Screen Reader |
| Keyboard Navigation | – | Alle Funktionen per Tastatur erreichbar |

---

## 9. Text-Chat Features

### Emoji

| Attribut | Wert |
|----------|------|
| **Standard** | Unicode 15.0 |
| **Rust Library** | `emojis` |
| **Lizenz** | MIT/Apache 2.0 |

### Link Previews

| Attribut | Wert |
|----------|------|
| **Standard** | Open Graph Protocol (ogp.me) |
| **Fallback** | oEmbed (oembed.com) |
| **Rust Library** | Custom Implementation |

### Media Types

| Attribut | Wert |
|----------|------|
| **Standard** | MIME Types (IETF RFC 6838) |
| **Rust Library** | `mime` |
| **Lizenz** | MIT/Apache 2.0 |

---

## 10. Vollständige Dependency-Liste

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

## 11. Compliance-Tooling

### cargo-deny

Automatische Lizenzprüfung in CI/CD:

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

## Referenzen

- [PROJECT_SPEC.md](../project/specification.md) - Projektanforderungen
- [ARCHITECTURE.md](../architecture/overview.md) - Technische Architektur
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) - Detaillierte Lizenzprüfung
