# VoiceChat

A self-hosted voice and text chat platform for gaming communities.

<!-- CI badge: configure after GitHub Actions setup -->
<!-- [![CI](https://github.com/OWNER/REPO/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/REPO/actions/workflows/ci.yml) -->
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

## Features

- **Low Latency Voice Chat** – WebRTC-based with Opus codec, optimized for gaming
- **End-to-End Encryption** – Text messages encrypted with Olm/Megolm
- **Self-Hosted** – Your data stays on your server
- **Lightweight Client** – Tauri-based desktop app with minimal resource usage
- **SSO Support** – Integrate with Authentik, Keycloak, Azure AD, and more
- **Open Source** – MIT/Apache-2.0 dual licensed

## Architecture

### System Overview

```mermaid
graph TB
    subgraph Client["Tauri Desktop Client"]
        WebView["Solid.js WebView<br/>(UI, Stores, Routing)"]
        RustCore["Rust Core"]
        WebView <-->|"invoke()"| RustCore

        subgraph RustModules["Rust Modules"]
            AudioEngine["Audio Engine<br/>(cpal + opus)"]
            WebRTC_C["WebRTC Client<br/>(webrtc-rs)"]
            CryptoClient["E2EE Crypto<br/>(vodozemac)"]
            WSClient["WebSocket Client"]
            Capture["Screen Capture"]
        end
        RustCore --- RustModules
    end

    subgraph Server["Rust Server (axum + tokio)"]
        Router["HTTP Router<br/>(REST API)"]
        WSServer["WebSocket Server"]
        SFU["Voice SFU<br/>(Selective Forwarding)"]
        AuthService["Auth Service<br/>(JWT, OIDC, MFA)"]
        ChatService["Chat Service<br/>(Messages, Uploads)"]
        PermService["Permission Resolver<br/>(Roles, Overrides)"]
        KeyService["Key Service<br/>(E2EE Key Distribution)"]
        PresenceService["Presence Service"]
        AdminService["Admin Service<br/>(Elevated Sessions)"]
    end

    subgraph DataLayer["Data Layer"]
        PG[("PostgreSQL<br/>(Persistent State)")]
        Redis[("Valkey / Redis<br/>(Pub/Sub, Cache, Rate Limits)")]
        S3[("S3 Storage<br/>(File Attachments)")]
    end

    subgraph ExtAuth["External Identity Providers"]
        OIDC_P["OIDC / OAuth2<br/>(GitHub, Google, Keycloak, ...)"]
    end

    WebView -->|"HTTPS REST"| Router
    WSClient <-->|"WSS (TLS 1.3)"| WSServer
    WebRTC_C <-->|"DTLS-SRTP (Opus RTP)"| SFU
    AuthService <-->|"OAuth2/OIDC"| OIDC_P

    Router --- AuthService
    Router --- ChatService
    Router --- PermService
    Router --- KeyService
    Router --- AdminService
    WSServer --- PresenceService
    WSServer --- ChatService

    AuthService --> PG
    ChatService --> PG
    ChatService --> S3
    PermService --> PG
    KeyService --> PG
    WSServer --> Redis
    PresenceService --> Redis
    SFU --> Redis
    AuthService --> Redis
```

### Authentication Flow

#### Local Authentication (Username + Password)

```mermaid
sequenceDiagram
    participant C as Client
    participant S as Server
    participant DB as PostgreSQL
    participant R as Redis

    C->>S: POST /auth/register {username, email, password}
    S->>S: Validate input, check registration_policy
    S->>S: Hash password (Argon2id)
    S->>DB: INSERT user (auth_method='local')
    S-->>C: 201 {user_id}

    C->>S: POST /auth/login {username, password}
    S->>DB: SELECT user by username
    S->>S: Verify Argon2id hash

    alt MFA Enabled
        S-->>C: 200 {mfa_required: true, mfa_token}
        C->>S: POST /auth/mfa/verify {mfa_token, code}
        S->>S: Verify TOTP code
    end

    S->>S: Sign JWT (EdDSA/RS256, 15min expiry)
    S->>DB: INSERT session {token_hash, ip, user_agent}
    S-->>C: 200 {access_token, refresh_token}

    Note over C,S: Subsequent requests
    C->>S: GET /api/... [Authorization: Bearer <jwt>]
    S->>S: Verify JWT signature + expiry

    Note over C,S: Token refresh
    C->>S: POST /auth/refresh {refresh_token}
    S->>DB: Validate session, rotate token
    S-->>C: 200 {access_token, refresh_token}
```

#### SSO / OIDC Authentication

```mermaid
sequenceDiagram
    participant C as Client (Tauri)
    participant B as System Browser
    participant S as Server
    participant R as Redis
    participant IDP as Identity Provider

    C->>S: GET /auth/oidc/providers
    S-->>C: [{slug, display_name, icon_hint}]

    C->>C: Bind TCP on 127.0.0.1:{random_port}
    C->>S: GET /auth/oidc/authorize/{slug}?redirect_uri=http://127.0.0.1:{port}/callback

    S->>S: Generate PKCE (S256), state (32B), nonce
    S->>S: Encrypt flow state (AES-256-GCM)
    S->>R: SET oidc:state:{sha256(state)} = encrypted_flow (TTL 600s)
    S-->>C: 302 → IDP authorize URL

    C->>B: Open redirect URL in browser
    B->>IDP: User authenticates + consents
    IDP-->>B: 302 → /auth/oidc/callback?code=...&state=...

    B->>S: GET /auth/oidc/callback?code=...&state=...
    S->>R: GETDEL oidc:state:{sha256(state)}
    S->>S: Decrypt + verify flow state
    S->>IDP: POST token endpoint {code, PKCE verifier}
    IDP-->>S: {access_token, id_token}
    S->>S: Verify ID token (signature + nonce)
    S->>IDP: GET userinfo endpoint
    IDP-->>S: {sub, email, name, preferred_username}

    S->>S: Resolve user (external_id = slug:subject)
    alt New User
        S->>S: Generate username from claims
        S->>S: Create user in transaction (first-user → admin)
    end
    S->>S: Issue JWT + session
    S-->>B: 302 → http://127.0.0.1:{port}/callback?access_token=...

    B->>C: TCP callback with tokens
    C->>C: Store tokens, init WebSocket
```

### Voice & WebRTC Flow

#### Joining a Voice Channel

```mermaid
sequenceDiagram
    participant C as Client
    participant WS as WebSocket
    participant SFU as SFU Server
    participant P as Other Peers

    C->>C: Init audio engine (cpal, Opus 48kHz)
    C->>C: Create RTCPeerConnection (ICE servers)
    C->>C: Create local audio track + video track (placeholder)
    C->>WS: ClientEvent::VoiceJoin {channel_id}

    WS->>SFU: handle_join(user_id, channel_id)
    SFU->>SFU: Get or create Room
    SFU->>SFU: Create Peer + RTCPeerConnection
    SFU->>SFU: Create subscriber tracks for existing peers

    SFU-->>WS: ServerEvent::VoiceOffer {sdp}
    WS-->>C: VoiceOffer

    C->>C: Set remote description (offer)
    C->>C: Create SDP answer
    C->>WS: ClientEvent::VoiceAnswer {sdp}
    WS->>SFU: Set remote description (answer)

    loop ICE Trickle
        C->>WS: VoiceIceCandidate {candidate}
        WS->>SFU: Add ICE candidate
        SFU-->>WS: VoiceIceCandidate {candidate}
        WS-->>C: Add ICE candidate
    end

    Note over C,SFU: DTLS-SRTP established
    C->>SFU: Opus RTP packets (encrypted)
    SFU->>SFU: Route tracks to all peers in Room
    SFU->>P: Forward RTP packets

    SFU-->>WS: VoiceRoomState {participants, screen_shares}
    WS-->>C: Update participant list

    par Broadcast to room
        SFU-->>P: ServerEvent::VoiceUserJoined {user_id, username}
    end
```

#### SFU Track Routing

```mermaid
graph LR
    subgraph Room["Voice Room (Channel)"]
        subgraph PeerA["Peer A"]
            A_Mic["🎤 Mic Track"]
            A_Screen["🖥️ Screen Track"]
        end
        subgraph PeerB["Peer B"]
            B_Mic["🎤 Mic Track"]
        end
        subgraph PeerC["Peer C"]
            C_Mic["🎤 Mic Track"]
        end

        subgraph TrackRouter["Track Router (SFU)"]
            Router_Core["Selective<br/>Forwarding"]
        end

        A_Mic -->|RTP| Router_Core
        A_Screen -->|RTP| Router_Core
        B_Mic -->|RTP| Router_Core
        C_Mic -->|RTP| Router_Core

        Router_Core -->|"A's audio + screen"| PeerB
        Router_Core -->|"A's audio + screen"| PeerC
        Router_Core -->|"B's audio"| PeerA
        Router_Core -->|"B's audio"| PeerC
        Router_Core -->|"C's audio"| PeerA
        Router_Core -->|"C's audio"| PeerB
    end
```

#### DM Voice Calls (Event-Sourced)

```mermaid
stateDiagram-v2
    [*] --> Ringing: CallStarted
    Ringing --> Active: UserJoined (callee accepts)
    Ringing --> Ended: AllDeclined / Timeout
    Ringing --> Ended: CallerLeft
    Active --> Active: UserJoined / UserLeft
    Active --> Ended: LastParticipantLeft
    Active --> Ended: ExplicitEnd

    state Ringing {
        [*] --> WaitingForAnswer
        WaitingForAnswer --> PartialDecline: UserDeclined
        PartialDecline --> WaitingForAnswer: Other targets remain
    }

    state Active {
        [*] --> InCall
        InCall --> InCall: Participants change
    }

    note right of Ringing
        State stored in Redis Streams
        Events: Started, Joined, Left, Declined, Ended
    end note
```

### Chat Message Flow

```mermaid
sequenceDiagram
    participant C as Sender Client
    participant API as REST API
    participant DB as PostgreSQL
    participant S3 as S3 Storage
    participant R as Redis Pub/Sub
    participant WS as WebSocket Server
    participant O as Other Clients

    C->>C: User types message

    alt E2EE Channel (DMs)
        C->>C: Encrypt with Megolm session
        C->>C: Set encrypted=true, add nonce
    end

    alt Has File Attachment
        C->>API: POST /api/messages/channel/{id}/upload (multipart)
        API->>API: Validate MIME type + file size
        API->>API: Sanitize filename
        API->>DB: INSERT message
        API->>S3: Upload file (attachments/{channel}/{msg}/{uuid}.ext)
        API->>DB: INSERT file_attachment
    else Text Only
        C->>API: POST /api/messages/channel/{id} {content}
        API->>API: Validate (1-4000 chars)
        API->>API: Detect mention_type (@everyone, @here, @user)
        API->>DB: INSERT message
    end

    API->>R: PUBLISH channel:{channel_id} MessageNew
    API-->>C: 201 {message}

    R-->>WS: Message event
    WS-->>O: ServerEvent::MessageNew {channel_id, message}

    O->>O: addMessage() to store

    alt Encrypted Message
        O->>O: Decrypt with Megolm session key
    end

    O->>O: Play notification sound (based on mention_type)
    O->>O: Increment unread count

    Note over C,O: Editing
    C->>API: PATCH /api/messages/{id} {content}
    API->>DB: UPDATE message, set edited_at
    API->>R: PUBLISH MessageEdit
    R-->>WS: Forward
    WS-->>O: ServerEvent::MessageEdit

    Note over C,O: Deletion
    C->>API: DELETE /api/messages/{id}
    API->>DB: SET deleted_at (soft delete)
    API->>R: PUBLISH MessageDelete
    R-->>WS: Forward
    WS-->>O: ServerEvent::MessageDelete
```

### End-to-End Encryption (E2EE)

#### Key Distribution (Olm/Megolm via vodozemac)

```mermaid
sequenceDiagram
    participant A as User A (Client)
    participant S as Key Server
    participant DB as PostgreSQL
    participant B as User B (Client)

    Note over A: First-time setup
    A->>A: Generate Ed25519 identity key
    A->>A: Generate Curve25519 signed prekey
    A->>A: Generate batch of one-time prekeys
    A->>S: POST /api/keys/upload {identity_key, signed_prekey, one_time_keys}
    S->>DB: UPSERT user_keys

    Note over A,B: Starting encrypted DM
    A->>S: POST /api/keys/claim {user_id: B}
    S->>DB: Pop one-time prekey from B's bundle
    S-->>A: {identity_key, signed_prekey, one_time_key}

    A->>A: Create Olm session (X3DH key agreement)
    A->>A: Create Megolm outbound session (for group ratchet)
    A->>A: Encrypt Megolm session key with Olm
    A->>B: Send encrypted Megolm key (via message)

    Note over A,B: Sending messages
    A->>A: Encrypt message with Megolm session
    A->>S: POST message {content: ciphertext, encrypted: true, nonce}
    S->>B: Forward via WebSocket
    B->>B: Decrypt with Megolm session key

    Note over A: Key replenishment
    A->>S: GET /api/keys/count
    S-->>A: {remaining: 2}
    A->>A: Generate new one-time prekeys
    A->>S: POST /api/keys/upload {one_time_keys: [...]}
```

#### Encryption Layers

```mermaid
graph TB
    subgraph TextE2EE["Text E2EE (Olm/Megolm)"]
        direction TB
        Olm["Olm (1:1 Sessions)<br/>X3DH Key Agreement<br/>Double Ratchet"]
        Megolm["Megolm (Group Sessions)<br/>Sender Ratchet<br/>Shared Session Key"]
        Olm -->|"Key exchange"| Megolm
        Megolm -->|"Encrypt messages"| EncMsg["Encrypted Message<br/>{ciphertext, nonce}"]
    end

    subgraph VoiceEnc["Voice Encryption"]
        DTLS["DTLS-SRTP<br/>(Server-Trusted)<br/>Key exchange per peer"]
        SRTP["SRTP<br/>Opus packets encrypted<br/>in transit"]
        DTLS --> SRTP
    end

    subgraph AtRest["Secrets at Rest"]
        AES["AES-256-GCM"]
        MFA_Secrets["MFA Secrets<br/>(TOTP seeds)"]
        OIDC_Secrets["OIDC Client Secrets"]
        OIDC_Flow["OIDC Flow State<br/>(PKCE verifier, nonce)"]
        AES --- MFA_Secrets
        AES --- OIDC_Secrets
        AES --- OIDC_Flow
    end

    subgraph Transport["Transport Security"]
        TLS["TLS 1.3<br/>(rustls)"]
        WSS["WebSocket (WSS)"]
        HTTPS["HTTPS (REST API)"]
        TLS --- WSS
        TLS --- HTTPS
    end

    subgraph Keys["Key Material"]
        IdKey["Ed25519<br/>Identity Key"]
        PreKey["Curve25519<br/>Signed Prekey"]
        OTK["One-Time Prekeys<br/>(Curve25519)"]
        JWT_Key["JWT Signing Key<br/>(EdDSA / RS256)"]
        MFA_Key["MFA Encryption Key<br/>(AES-256)"]
    end
```

### Permission System

```mermaid
graph TB
    subgraph Resolution["Permission Resolution"]
        direction TB
        User["User"] --> OwnerCheck{"Is Guild<br/>Owner?"}
        OwnerCheck -->|Yes| AllPerms["Grant ALL permissions"]
        OwnerCheck -->|No| EveryoneBase["Start with @everyone<br/>role permissions"]

        EveryoneBase --> MemberRoles["OR assigned role permissions"]
        MemberRoles --> Role1["Role: Admin<br/>permissions: 0x3FFFF"]
        MemberRoles --> Role2["Role: Moderator<br/>permissions: 0x3C00"]
        Role1 --> Combine["Bitwise OR<br/>into base permissions"]
        Role2 --> Combine

        Combine --> HasOverrides{"Channel<br/>context?"}
        HasOverrides -->|No| FinalPerms["Final Permissions"]
        HasOverrides -->|Yes| ChannelOverride["Apply Channel Overrides<br/>(per matching role)"]

        ChannelOverride --> Allow["Add ALLOW bits<br/>(perms |= allow)"]
        Allow --> Deny["Remove DENY bits<br/>(perms &= !deny)<br/>Deny wins"]
        Deny --> FinalPerms
    end

    subgraph Bits["Permission Bitflags (u64)"]
        direction LR
        subgraph Content["Content (0-4)"]
            B0["0: SEND_MESSAGES"]
            B1["1: EMBED_LINKS"]
            B2["2: ATTACH_FILES"]
            B3["3: USE_EMOJI"]
            B4["4: ADD_REACTIONS"]
        end
        subgraph Voice["Voice (5-9)"]
            B5["5: VOICE_CONNECT"]
            B6["6: VOICE_SPEAK"]
            B7["7: VOICE_MUTE_OTHERS"]
            B8["8: VOICE_DEAFEN_OTHERS"]
            B9["9: VOICE_MOVE_MEMBERS"]
        end
        subgraph Moderation["Moderation (10-13)"]
            B10["10: MANAGE_MESSAGES"]
            B11["11: TIMEOUT_MEMBERS"]
            B12["12: KICK_MEMBERS"]
            B13["13: BAN_MEMBERS"]
        end
        subgraph Management["Guild Mgmt (14-23)"]
            B14["14: MANAGE_CHANNELS"]
            B15["15: MANAGE_ROLES"]
            B17["17: MANAGE_GUILD"]
            B22["22: SCREEN_SHARE"]
            B23["23: MENTION_EVERYONE"]
        end
    end

    subgraph SystemAdmin["System Administration"]
        SA["System Admin<br/>(server-wide)"]
        ES["Elevated Session<br/>(sudo-style, time-limited)"]
        SA --> ES
        ES -->|"Required for"| AdminOps["Admin CRUD<br/>OIDC Management<br/>User Management"]
    end
```

### Real-Time Communication

```mermaid
graph TB
    subgraph WSProtocol["WebSocket Protocol"]
        direction TB
        Auth["Connection Auth<br/>Sec-WebSocket-Protocol: access_token.{jwt}"]

        subgraph ClientEvents["Client → Server"]
            CE1["Ping"]
            CE2["Subscribe / Unsubscribe {channel_id}"]
            CE3["Typing / StopTyping {channel_id}"]
            CE4["VoiceJoin / VoiceLeave"]
            CE5["VoiceAnswer / VoiceIceCandidate"]
            CE6["VoiceMute / VoiceUnmute"]
            CE7["VoiceScreenShareStart / Stop"]
            CE8["VoiceStats {latency, packet_loss, jitter}"]
            CE9["SetActivity {activity: Option&lt;Activity&gt;}"]
            CE10["AdminSubscribe"]
        end

        subgraph ServerEvents["Server → Client"]
            SE1["Ready {user_id} / Pong"]
            SE2["MessageNew / MessageEdit / MessageDelete"]
            SE3["ReactionAdd / ReactionRemove"]
            SE4["TypingStart / TypingStop"]
            SE5["VoiceOffer / VoiceIceCandidate"]
            SE6["VoiceUserJoined / Left / Muted / Unmuted"]
            SE7["VoiceRoomState / VoiceError / VoiceUserStats"]
            SE8["ScreenShareStarted / Stopped / QualityChanged"]
            SE9["PresenceUpdate / RichPresenceUpdate"]
            SE10["Patch {entity_type, entity_id, diff}"]
            SE11["FriendRequestReceived / Accepted"]
            SE12["GuildEmojiUpdated"]
        end
    end

    subgraph PubSub["Redis Pub/Sub Channels"]
        CH["channel:{channel_id}<br/>Message + typing events"]
        PR["presence:{user_id}<br/>Status + activity updates"]
        US["user:{user_id}<br/>Cross-device sync"]
        GU["guild:{guild_id}<br/>Guild-wide updates"]
        VO["voice:{channel_id}<br/>Call state (Redis Streams)"]
    end

    ClientEvents -->|"Processed by<br/>server handlers"| PubSub
    PubSub -->|"Broadcast to<br/>subscribed clients"| ServerEvents
```

### Client Architecture

```mermaid
graph TB
    subgraph TauriApp["Tauri Desktop Application"]
        subgraph SolidJS["Solid.js WebView"]
            subgraph Views["Views (Routes)"]
                Login["Login / Register"]
                Main["Main Chat UI"]
                Admin["Admin Dashboard"]
            end

            subgraph Components["Components"]
                ChatComp["Chat: MessageList,<br/>MessageInput, Reactions"]
                GuildComp["Guild: Sidebar,<br/>ChannelList, Settings"]
                VoiceComp["Voice: Controls,<br/>Participants, ScreenShare"]
                UIComp["UI: Modals, Buttons,<br/>Toasts, Tooltips"]
                AdminComp["Admin: UserMgmt,<br/>Settings, OIDC Config"]
            end

            subgraph Stores["Reactive Stores (Signals)"]
                AuthStore["auth.ts<br/>JWT, user, session"]
                WSStore["websocket.ts<br/>Connection, event routing"]
                MsgStore["messages.ts<br/>byChannel: Record<id, Message[]>"]
                ChanStore["channels.ts<br/>Channel list, unread counts"]
                GuildStore["guilds.ts<br/>Guild data, navigation"]
                VoiceStore["voice.ts<br/>Connection state, peers"]
                PresStore["presence.ts<br/>User status, activities"]
                E2EEStore["e2ee.ts<br/>Key management"]
                PrefStore["preferences.ts<br/>Theme, notifications"]
            end

            Views --> Components
            Components --> Stores
        end

        subgraph RustBackend["Rust Backend"]
            AudioCmd["Audio Commands<br/>input/output device control"]
            VoiceCmd["Voice Commands<br/>join, leave, mute"]
            CryptoCmd["Crypto Commands<br/>Olm/Megolm operations"]
            WSCmd["WebSocket Commands<br/>connect, send"]
            CaptureCmd["Capture Commands<br/>Screen share"]
            OIDCCmd["OIDC Commands<br/>SSO flow (TCP callback)"]
        end

        Stores -->|"invoke()"| RustBackend
        RustBackend -->|"Tauri events"| WSStore
    end
```

### Data Layer

```mermaid
erDiagram
    users ||--o{ guild_members : joins
    users ||--o{ messages : sends
    users ||--o{ sessions : has
    users ||--o| user_keys : "has E2EE keys"
    users ||--o| system_admins : "may be admin"

    guilds ||--o{ guild_members : contains
    guilds ||--o{ channels : has
    guilds ||--o{ guild_roles : defines
    guilds ||--|| users : "owned by"

    channels ||--o{ messages : contains
    channels ||--o{ channel_overrides : has
    channels }o--o| channel_categories : "grouped in"

    messages ||--o{ file_attachments : has
    messages ||--o{ reactions : has
    messages }o--o| messages : "replies to"

    guild_roles ||--o{ guild_member_roles : assigned
    guild_roles ||--o{ channel_overrides : overrides
    guild_member_roles }o--|| users : "assigned to"

    sessions ||--o| elevated_sessions : "may elevate"

    users {
        uuid id PK
        varchar username UK
        varchar email UK
        varchar password_hash
        enum auth_method "local | oidc"
        varchar external_id UK
        enum status "online | away | busy | offline"
        jsonb activity
        varchar mfa_secret
    }

    guilds {
        uuid id PK
        varchar name
        uuid owner_id FK
        jsonb security_settings
    }

    channels {
        uuid id PK
        varchar name
        enum channel_type "text | voice | dm"
        uuid guild_id FK
        varchar icon
        int position
    }

    messages {
        uuid id PK
        uuid channel_id FK
        uuid user_id FK
        text content
        boolean encrypted
        varchar nonce
        uuid reply_to FK
        timestamptz deleted_at
    }

    guild_roles {
        uuid id PK
        uuid guild_id FK
        varchar name
        bigint permissions "bitflags"
        int position
        boolean is_default
    }

    oidc_providers {
        uuid id PK
        varchar slug UK
        varchar display_name
        text issuer_url
        varchar client_id
        text client_secret_encrypted
        boolean enabled
    }

    file_attachments {
        uuid id PK
        uuid message_id FK
        varchar filename
        varchar mime_type
        bigint size_bytes
        varchar s3_key
    }
```

### Request Lifecycle

```mermaid
flowchart LR
    Req["Incoming Request"] --> TLS["TLS 1.3<br/>(rustls)"]
    TLS --> Rate["Rate Limiter<br/>(Token Bucket)"]
    Rate --> Auth["JWT Verification<br/>(EdDSA / RS256)"]
    Auth --> Perm["Permission Check<br/>(Bitflags + Overrides)"]
    Perm --> Handler["Route Handler"]
    Handler --> DB["PostgreSQL"]
    Handler --> Cache["Redis Cache"]
    Handler --> S3_H["S3 (if upload)"]
    Handler --> Broadcast["Redis Pub/Sub<br/>(real-time events)"]
    Broadcast --> WS_Deliver["WebSocket Delivery<br/>(to subscribed clients)"]
    Handler --> Resp["JSON Response"]
```

## Documentation

All documentation is located in the [`docs/`](docs/) directory.

### Getting Started
- [System Dependencies](docs/getting-started/dependencies.md)
- [Development Setup](docs/development/setup.md)

### Operations
- [Configuration Guide](docs/ops/configuration.md)
- [Deployment Guide](docs/ops/deployment.md)

### Architecture & Security
- [Architecture Overview](docs/architecture/overview.md)
- [Encryption Architecture](docs/security/encryption.md)

### Project
- [Roadmap](docs/project/roadmap.md)
- [Design Guidelines](docs/design/ux-guidelines.md)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
