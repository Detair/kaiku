# Kaiku Platform - Technical Architecture

## Architecture Overview

```mermaid
flowchart TD
    subgraph ClientLayer ["CLIENT LAYER"]
        direction LR
        Win["Windows (Tauri 2.0)\nWebView (Solid.js)\nRust Core (WebRTC, Audio, Crypto)"]
        Lin["Linux (Tauri 2.0)\nWebView (Solid.js)\nRust Core (WebRTC, Audio, Crypto)"]
        Mac["macOS (Tauri 2.0)\nWebView (Solid.js)\nRust Core (WebRTC, Audio, Crypto)"]
    end

    Internet(("INTERNET\n(TLS 1.3 encrypted)"))
    
    Win <--> Internet
    Lin <--> Internet
    Mac <--> Internet

    subgraph ServerLayer ["SERVER LAYER"]
        direction TB
        Gateway["API Gateway\n(Reverse Proxy)\n• TLS Termination\n• Rate Limiting\n• Load Balancing"]
        
        subgraph Services [" "]
            direction LR
            Auth["Auth Service\n• Local Auth\n• OIDC/SSO\n• MFA\n• Sessions"]
            Chat["Chat Service\n• Channels\n• Messages\n• File Upload\n• E2EE"]
            Voice["Voice Service (SFU)\n• webrtc-rs\n• Opus Codec\n• DTLS-SRTP"]
        end
        
        Gateway <--> Auth
        Gateway <--> Chat
        Gateway <--> Voice

        subgraph DataLayer ["Data Layer"]
            direction LR
            PG[("PostgreSQL\n• Users, Channels,\nMessages, Permissions")]
            VK[("Valkey\n• Sessions, Presence,\nPub/Sub")]
            S3[("S3 Storage\n• Files, Avatars,\nBackups")]
        end
        
        Auth <--> DataLayer
        Chat <--> DataLayer
        Voice <--> DataLayer
    end

    Internet <--> Gateway
```

---

## Component Details

### 1. Client Architecture (Tauri 2.0)

```mermaid
flowchart TD
    subgraph TauriClient ["TAURI CLIENT"]
        direction TB
        subgraph Frontend ["FRONTEND (WebView: Solid.js, UnoCSS)"]
            direction LR
            Views["Views\n• Login\n• Channels\n• Settings\n• Voice"]
            Comps["Components\n• Channel\n• Message\n• UserList\n• VoiceBar"]
            Stores["Stores\n• Auth\n• Channels\n• Messages\n• Voice"]
            Views --- Comps --- Stores
        end

        TauriCmds{{"Tauri Commands"}}

        subgraph Backend ["BACKEND (Rust)"]
            direction LR
            Audio["Audio\n• cpal\n• opus"]
            WebRTC["WebRTC\n• webrtc-rs\n• Signaling\n• DTLS-SRTP"]
            Crypto["Crypto\n• vodozemac\n• Key Store\n• Keyring"]
            Net["Network\n• HTTP/REST\n• WebSocket\n• rustls"]
            Audio --- WebRTC --- Crypto --- Net
        end

        Frontend <-->|TauriCmds| Backend
    end
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

```mermaid
flowchart TD
    subgraph Chat [CHAT SERVICE]
        direction TB
        R1["REST:\nGET/POST/PATCH/DELETE /channels\nGET/POST/PATCH/DELETE /messages\nPOST /upload"]
        W1["WS Events:\n→ message.new/edit/delete\n→ typing.start/stop\n→ presence.update\n→ channel.update"]
        E1["E2EE:\n• Olm (1:1 DMs)\n• Megolm (Groups)"]
        
        R1 ~~~ W1 ~~~ E1
    end
```

#### Voice Service (SFU)

```mermaid
flowchart TD
    subgraph Voice [VOICE SERVICE SFU]
        direction TB
        ClientA -->|Offer| SFUServer
        SFUServer -->|Answer| ClientA
        ClientA == Media ==> SFUServer == Media ==> ClientB
        ClientB == Media ==> SFUServer == Media ==> ClientA
        
        AP["Pipeline:\nCapture → Opus Encode → SRTP Encrypt → Network\nNetwork → SRTP Decrypt → Opus Decode → Playback"]
    end
```

---

### 3. Database Schema (Overview)

```mermaid
erDiagram
    users ||--o{ sessions : has
    users ||--o{ user_keys : owns
    channels ||--o{ channel_members : contains
    users ||--o{ channel_members : joins
    roles ||--o{ channel_members : "assigned to"
    channels ||--o{ messages : contains
    users ||--o{ messages : sends
    channels ||--o{ megolm_sessions : has
    messages ||--o{ files : has
```

---

### 4. Encryption Architecture

```mermaid
flowchart TD
    subgraph Trans ["LAYER 1: Transport"]
        Client <-->|TLS 1.3| Server
    end
    subgraph VoiceE ["LAYER 2: Voice (WebRTC)"]
        ClientC <-->|DTLS-SRTP| SFU <-->|DTLS-SRTP| ClientD
    end
    subgraph Text ["LAYER 3: Text Messages"]
        UserA <-->|Olm Session| UserB
        UserE -->|Megolm Outbound| Shared
        Shared -->|Megolm Inbound| UserF
        Shared -->|Megolm Inbound| UserG
    end
    subgraph Rest ["LAYER 4: Data at Rest"]
        Stored["• Messages: E2EE encrypted\n• Files: AES-256-GCM\n• Backups: Encrypted"]
    end
```

---

### 5. SSO/Identity Architecture

```mermaid
flowchart TD
    Request["User Request: Login with SSO"] --> AuthSvc
    subgraph AuthSvc ["Auth Service"]
        Local["Local Auth"]
        OIDC["OIDC Handler"]
    end
    OIDC --> SSOProviders["Authentik, Keycloak, Azure AD, Okta, Google..."]
    Local --> UserStore
    SSOProviders --> UserStore["Unified User Store"]
```

---

### 6. Deployment Architecture

```mermaid
flowchart TD
    subgraph Docker ["Docker Network (voicechat_net)"]
        Traefik["Traefik (Proxy)\nPort 443 / 80"]
        Traefik --> API["kaiku-api (Auth + Chat)"]
        Traefik --> Voice["kaiku-voice (SFU, UDP 10000-10100)"]
        Traefik --> Web["kaiku-web (Static)"]
        
        API --> Valkey["Valkey"]
        API --> Postgres["Postgres"]
    end
```

---

### 7. Backup & Recovery

```mermaid
flowchart TD
    Cron["Cronjob (03:00)"] --> BackupScript
    BackupScript --> S3Bucket
    S3Bucket --> Encrypt["Encrypt AES-256"]
    Encrypt --> Delete["Delete after 30 days"]
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
- [LICENSE_COMPLIANCE.md](../../../LICENSE_COMPLIANCE.md) - License Review
