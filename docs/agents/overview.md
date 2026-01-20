# VoiceChat Platform (Canis)

## Purpose
Self-hosted voice and text chat platform for gaming communities. Optimized for low latency (<50ms), high voice quality, and maximum security.

**License:** MIT OR Apache-2.0 (Dual License)
**Stack:** Rust (Server + Tauri Client), Solid.js (Frontend), PostgreSQL, Redis

## Architecture Overview

```
Client (Tauri 2.0)          Server (Axum)
├── WebView (Solid.js)      ├── Auth Service (JWT, OIDC, MFA)
└── Rust Core               ├── Chat Service (WebSocket, E2EE)
    ├── WebRTC (webrtc-rs)  ├── Voice Service (SFU, DTLS-SRTP)
    └── Audio (cpal, opus)  └── Data Layer
                                ├── PostgreSQL
                                ├── Redis
                                └── S3 Storage
```

## Key Files
- `Cargo.toml` - Rust workspace configuration
- `CLAUDE.md` - AI agent instructions and code review guidelines
- `CHANGELOG.md` - Change log (keepachangelog.com format)
- `README.md` - Quick start and project overview
- `Makefile` - Build and development commands
- `docker-compose.dev.yml` - Development services (PostgreSQL, Redis, MinIO)
- `deny.toml` - License compliance (cargo-deny configuration)
- `.env.example` - Environment configuration template

## Subdirectories
- `server/` - Backend server (Rust/Axum) - see server/AGENTS.md
- `client/` - Desktop client (Tauri 2.0 + Solid.js) - see client/AGENTS.md
- `shared/` - Shared Rust libraries (types, crypto) - see shared/AGENTS.md
- `infra/` - Infrastructure (Docker, scripts) - see infra/AGENTS.md
- `docs/` - Documentation and design plans - see docs/AGENTS.md
- `specs/` - Project specifications - see specs/AGENTS.md
- `scripts/` - Build and utility scripts - see scripts/AGENTS.md
- `.claude/` - Claude Code configuration - see .claude/AGENTS.md
- `.github/` - GitHub Actions CI/CD workflows - see .github/AGENTS.md

## For AI Agents

### Critical Constraints
1. **License Compliance**: Run `cargo deny check licenses` before adding dependencies. FORBIDDEN: GPL, AGPL, LGPL (static linking)
2. **Performance Targets**: Voice latency <50ms, Client RAM <80MB idle, Startup <3s
3. **Security**: TLS 1.3 everywhere, Argon2id for passwords, JWT 15min expiry

### Key Decisions
| Area | Choice | Rationale |
|------|--------|-----------|
| Text E2EE | vodozemac (Olm/Megolm) | Apache 2.0 (libsignal is AGPL) |
| Voice MVP | DTLS-SRTP | Standard WebRTC, Server-trusted |
| Client | Tauri 2.0 + Solid.js | <100MB RAM vs Discord ~400MB |
| IDs | UUIDv7 | Time-sortable, decentralized |

### Code Style
- Error Handling: `thiserror` for libraries, `anyhow` for applications
- Async: tokio with `tracing` instrumentation
- Package Manager: Bun (frontend), Cargo (Rust)

### Useful Commands
```bash
make dev          # Start server in watch mode
make client       # Start client in dev mode
make test         # Run all tests
make check        # Cargo check + clippy
cargo deny check  # License compliance check
```

## Dependencies
- PostgreSQL 15+ (primary database)
- Redis 7+ (sessions, caching, presence)
- MinIO (S3-compatible file storage)
