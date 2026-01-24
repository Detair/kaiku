# VoiceChat Platform - Project Specification

## Project Overview

A self-hosted voice and text chat platform for gaming communities, optimized for low latency, high voice quality, and maximum security.

**Project Status:** Planning Phase
**License:** MIT OR Apache-2.0 (Dual License)
**Target Audience:** Gaming communities, self-hosters, organizations with privacy requirements

---

## Core Objectives

| Priority | Goal | Description |
|----------|------|-------------|
| 🔴 High | Low Latency | Voice chat must be suitable for gaming (<50ms) |
| 🔴 High | High Voice Quality | Clear communication even with many participants |
| 🔴 High | Security | End-to-end encryption, secure server architecture |
| 🔴 High | Low Resource Usage | Client must not impact gaming performance |
| 🟡 Medium | Self-Hosted First | Easy deployment with Docker |
| 🟡 Medium | Modularity | Extensible through themes and plugins |
| 🟢 Low | Mobile Clients | Android/iOS as bonus later |
| 🟢 Low | SaaS Option | Architecture should enable SaaS later |

---

## Functional Requirements

### Voice Chat

| Feature | Priority | MVP | Details |
|---------|----------|-----|---------|
| Real-time Voice | 🔴 High | ✅ | WebRTC-based, Opus codec |
| Push-to-Talk | 🔴 High | ✅ | Configurable hotkeys |
| Voice Activation | 🔴 High | ✅ | Adjustable thresholds |
| Noise Cancellation | 🔴 High | ✅ | RNNoise-based |
| Echo Cancellation | 🔴 High | ✅ | WebRTC AEC |
| Volume Control | 🟡 Medium | ✅ | Adjustable per user |
| Spatial Audio | 🟢 Low | ❌ | Interesting for gaming, later |

### Text Chat

| Feature | Priority | MVP | Details |
|---------|----------|-----|---------|
| Text Channels | 🔴 High | ✅ | Separate channels like Discord/Slack |
| In-Voice Chat | 🔴 High | ✅ | Text chat within voice channels |
| Markdown Support | 🔴 High | ✅ | CommonMark standard |
| Emojis | 🔴 High | ✅ | Unicode + Custom Emojis |
| @Mentions | 🔴 High | ✅ | User and channel mentions |
| Image Uploads | 🔴 High | ✅ | With preview |
| Link Previews | 🟡 Medium | ✅ | Open Graph meta tags |
| File Uploads | 🟡 Medium | ✅ | Configurable limits |
| Message History | 🔴 High | ✅ | Searchable |
| Edit Message | 🟡 Medium | ✅ | With edit indicator |
| Delete Message | 🟡 Medium | ✅ | Soft-delete |
| Threads | 🟢 Low | ❌ | Later |
| Reactions | 🟢 Low | ❌ | Later |

### User Management

| Feature | Priority | MVP | Details |
|---------|----------|-----|---------|
| Local Users | 🔴 High | ✅ | Username + Password |
| SSO/OIDC | 🔴 High | ✅ | Authentik, Keycloak, Azure AD, etc. |
| MFA (TOTP) | 🔴 High | ✅ | Google Authenticator compatible |
| MFA (WebAuthn) | 🟡 Medium | ❌ | Hardware keys, later |
| Roles & Permissions | 🔴 High | ✅ | Fine-grained per channel |
| User Profiles | 🔴 High | ✅ | Avatar, status, bio |
| Online Status | 🔴 High | ✅ | Online, Away, Busy, Offline |
| Friends List | 🟡 Medium | ❌ | Later |
| Blocking | 🟡 Medium | ✅ | Users can block others |

### Server Structure

| Feature | Priority | MVP | Details |
|---------|----------|-----|---------|
| Multiple Voice Channels | 🔴 High | ✅ | With user limits |
| Multiple Text Channels | 🔴 High | ✅ | Categorizable |
| Channel Categories | 🟡 Medium | ✅ | Grouping |
| Private Channels | 🔴 High | ✅ | Permission-based |
| Temporary Channels | 🟢 Low | ❌ | Auto-delete when empty |

---

## Non-Functional Requirements

### Performance

| Metric | Target | Notes |
|--------|--------|-------|
| Voice Latency | <50ms | End-to-end |
| Client RAM | <100MB | Idle |
| Client CPU | <5% | With active voice |
| Server: 100 Users | 4 vCPU, 8GB RAM | Concurrent in voice |
| Client Startup Time | <3s | Until usable |

### Scaling

| Scenario | Support | Notes |
|----------|---------|-------|
| 10-100 Users (normal) | ✅ Single Node | Standard deployment |
| 100-500 Users (peak) | ✅ Single Node | More hardware |
| 500-1000+ Users | ⚠️ Cluster | Horizontal SFU scaling needed |

**Decision:** MVP focuses on single-node up to 100 users. Architecture allows later scaling.

### Security

| Requirement | Implementation |
|-------------|----------------|
| Transport Encryption | TLS 1.3 for all connections |
| Voice Encryption (MVP) | DTLS-SRTP (WebRTC standard) |
| Voice Encryption (later) | MLS for true E2EE ("Paranoid Mode") |
| Text Encryption | Olm/Megolm (vodozemac) |
| Password Hashing | Argon2id |
| Session Management | Opaque tokens + Redis |
| MFA | TOTP (RFC 6238) |

### Availability

| Requirement | Target |
|-------------|--------|
| Uptime | 99.9% (self-hosted depends on operator) |
| Backup Interval | Daily automatic |
| Recovery Time | <30 minutes from backup |
| Graceful Degradation | Text chat works during voice outage |

---

## Platform Support

### Server

| Platform | Support | Notes |
|----------|---------|-------|
| Linux (Docker) | 🔴 Primary | Ubuntu 22.04+ recommended |
| Linux (Native) | 🟡 Secondary | For advanced users |
| Windows | ❌ | Not planned |
| macOS | ❌ | Not planned |

### Desktop Clients

| Platform | Support | Framework |
|----------|---------|-----------|
| Windows 10/11 | 🔴 Primary | Tauri 2.0 |
| Linux | 🔴 Primary | Tauri 2.0 |
| macOS | 🔴 Primary | Tauri 2.0 |

### Mobile Clients (Bonus, later)

| Platform | Support | Framework |
|----------|---------|-----------|
| Android | 🟢 Bonus | Flutter or React Native |
| iOS | 🟢 Bonus | Flutter or React Native |

### Web Client

| Support | Notes |
|---------|-------|
| 🟡 Optional | Limited features possible via WebRTC in browser |

---

## Decision Log

### E-001: Encryption Strategy

**Date:** [Current Date]
**Status:** Decided

**Context:** Selection of encryption method for voice and text.

**Options:**
1. DTLS-SRTP + Signal Protocol
2. SFrame + Signal Protocol
3. MLS for both
4. Mesh/P2P

**Decision:**
- MVP: DTLS-SRTP (Voice) + Olm/Megolm via vodozemac (Text)
- Later optional: MLS for "Paranoid Mode"

**Rationale:**
- Minimal development effort for MVP
- Self-hosted = server is trusted
- vodozemac instead of libsignal due to AGPL license issue
- MLS architecture prepared for later upgrade

---

### E-002: SSO Handling

**Date:** [Current Date]
**Status:** Decided

**Context:** Integration of local users and SSO users.

**Decision:** "Unified Identity with Local Profile"
- JIT (Just-in-Time) provisioning for SSO users
- All users have local profile in the system
- Configurable attribute mapping
- Optional: Identity linking (local ↔ SSO)

**Rationale:**
- Unified user handling in the system
- Flexibility for different SSO providers
- No manual user creation needed

---

### E-003: Client Framework

**Date:** [Current Date]
**Status:** Decided

**Context:** Selection of framework for desktop clients.

**Options:**
1. Electron
2. Tauri
3. Flutter Desktop
4. Native (Qt, GTK)

**Decision:** Tauri 2.0

**Rationale:**
- Significantly lower RAM usage than Electron (~80MB vs ~300MB)
- Rust backend fits the server stack
- Native noise cancellation possible
- Cross-platform with one codebase
- MIT/Apache 2.0 licensed

---

### E-004: Project License

**Date:** [Current Date]
**Status:** Decided

**Context:** Choice of open-source license for the project.

**Options:**
1. MIT
2. Apache 2.0
3. MIT OR Apache 2.0 (Dual)
4. GPL 3.0
5. AGPL 3.0

**Decision:** MIT OR Apache-2.0 (Dual License)

**Rationale:**
- Maximum compatibility
- Standard in the Rust ecosystem
- Allows commercial use and SaaS
- Patent protection through Apache option
- No copyleft restrictions

---

### E-005: Text E2EE Library

**Date:** [Current Date]
**Status:** Decided

**Context:** libsignal is AGPL-licensed, which would force project license.

**Options:**
1. libsignal (AGPL 3.0)
2. vodozemac (Apache 2.0)
3. Custom implementation
4. OpenMLS (MIT)

**Decision:** vodozemac

**Rationale:**
- Apache 2.0 compatible with project license
- Implements Olm (1:1) and Megolm (groups)
- Production-tested by Matrix/Element
- Pure Rust, no C dependencies
- Double Ratchet with Perfect Forward Secrecy

---

### E-006: Scaling Strategy

**Date:** [Current Date]
**Status:** Decided

**Context:** Design for different user counts.

**Decision:**
- Phase 1 (MVP): Single-node with channel limits (50-100 per voice)
- Phase 2 (if needed): Horizontal SFU scaling

**Rationale:**
- 99% of use cases covered with single-node
- No over-engineering for MVP
- Architecture allows later upgrade

---

## Glossary

| Term | Definition |
|------|------------|
| SFU | Selective Forwarding Unit - Server that forwards media streams |
| DTLS | Datagram TLS - Encryption for UDP |
| SRTP | Secure RTP - Encrypted audio/video streaming |
| MLS | Message Layer Security - Modern E2EE standard for groups |
| Olm | Double Ratchet protocol for 1:1 E2EE |
| Megolm | Efficient group E2EE protocol |
| OIDC | OpenID Connect - SSO standard |
| JIT | Just-in-Time - Automatic user creation on first login |
| PFS | Perfect Forward Secrecy - Compromised keys don't endanger old messages |

---

## Next Steps

1. [ ] Finalize data model
2. [ ] API design (OpenAPI Spec)
3. [ ] Docker-Compose development environment
4. [ ] Project skeleton (Rust Workspace)
5. [ ] CI/CD pipeline setup
6. [ ] Define MVP feature scope
7. [ ] Milestones and timeline

---

## References

- [ARCHITECTURE.md](../architecture/overview.md) - Technical Architecture
- [STANDARDS.md](../development/standards.md) - Standards and Protocols Used
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) - License Compliance for All Dependencies
