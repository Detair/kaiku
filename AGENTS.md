# PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-27 · **Commit:** a5cfe9f · **Branch:** main

## OVERVIEW

Self-hosted voice/text-chat platform for gaming communities. Rust monorepo: Axum server, Tauri 2.0 desktop client, Solid.js frontend. PostgreSQL + Valkey. MIT OR Apache-2.0.

## STRUCTURE

```
./
├── server/             # Rust backend (axum, sqlx, webrtc-rs) → see server/AGENTS.md
├── client/             # Desktop app → see client/AGENTS.md
│   ├── src/            # Solid.js frontend (UnoCSS, Solid Router)
│   └── src-tauri/      # Rust backend (audio, crypto, WebRTC)
├── shared/             # Shared Rust crates → see shared/AGENTS.md
│   ├── vc-common/      # Types, WebSocket protocol (ClientEvent/ServerEvent)
│   └── vc-crypto/      # E2EE (vodozemac Olm/Megolm)
├── infra/              # Docker, Compose, Traefik → see infra/AGENTS.md
├── docs/               # Architecture, security, plans, roadmap
├── scripts/            # Dev setup, test runners → see scripts/AGENTS.md
└── .sqlx/              # Offline query cache — COMMIT when queries change
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add REST endpoint | `server/src/api/` | Router in `mod.rs`, feature in own file |
| Add WebSocket event | `shared/vc-common/src/protocol/` + `server/src/ws/` | Protocol change = BREAKING |
| Add Tauri command | `client/src-tauri/src/commands/` + register in `lib.rs` | Thin adapter pattern |
| Add UI component | `client/src/components/{domain}/` | Domain-organized |
| Add reactive store | `client/src/stores/` | `createStore` pattern, see existing |
| Add shared type | `shared/vc-common/src/types/` + re-export in `lib.rs` | Both server + client consume |
| Add migration | `server/migrations/` | `sqlx migrate add -r <name> --source server/migrations` |
| Change permissions | `server/src/permissions/` | Bitflags u64, deny wins over allow |
| Voice/WebRTC | `server/src/voice/` + `client/src-tauri/src/webrtc/` | <50ms latency target |
| Auth changes | `server/src/auth/` | Security review required |
| E2EE crypto | `shared/vc-crypto/` + `client/src-tauri/src/crypto/` | vodozemac only, NEVER custom crypto |
| Admin features | `server/src/admin/` + `client/src/components/admin/` | Elevated session required |
| Styling/themes | `client/src/styles/` + `client/uno.config.ts` | CSS variables, `data-theme` attribute |

## WORKSPACE

4-crate Rust workspace + Solid.js frontend:

| Crate | Type | Binary | Purpose |
|-------|------|--------|---------|
| `server/` | lib+bin | `vc-server` | REST API, WebSocket, SFU |
| `client/src-tauri/` | lib+bin | `vc-client` | Audio, crypto, WebRTC |
| `shared/vc-common/` | lib | — | Types, protocol |
| `shared/vc-crypto/` | lib | — | E2EE primitives |

Dependency graph (acyclic): server/client → vc-common, vc-crypto. Shared crates have NO internal deps.

## CONVENTIONS

**Rust:**
- `unsafe_code = "forbid"` — no unsafe anywhere
- Clippy: pedantic + nursery warnings (`[workspace.lints]` in Cargo.toml)
- rustfmt: 100-char lines, module-granularity imports, `StdExternalCrate` grouping
- Errors: `thiserror` for library, `anyhow` for application
- Observability: `#[tracing::instrument(skip(pool))]` on all public fns
- Modules: `mod.rs` + feature files per service

**TypeScript/Solid.js:**
- Strict mode, no unused locals/params
- Path alias: `@/*` → `./src/*`
- UnoCSS utility classes + CSS variable themes
- Props: NEVER destructure (breaks Solid.js reactivity)
- Lists: `<For>` not `.map()`

**Git:**
- NEVER commit to main — always feature branch + squash merge PR
- Branch: `feature/`, `fix/`, `refactor/`, `docs/`
- Commit: `type(scope): subject` (max 72 chars, imperative)
- Changelog: user-facing changes in `CHANGELOG.md` under `[Unreleased]`

**Builds:**
- SQLx offline: `SQLX_OFFLINE=true` in CI, commit `.sqlx/` on query changes
- Frontend package manager: **Bun** (not npm/yarn)
- License compliance: `cargo deny check licenses` — no GPL/AGPL/LGPL

## ANTI-PATTERNS (THIS PROJECT)

- **No `as any` / `@ts-ignore` / `@ts-expect-error`** — strict TS enforced
- **No unsafe Rust** — `forbid(unsafe_code)` workspace-wide
- **No GPL/AGPL dependencies** — `cargo deny` enforces
- **No tokens in localStorage** — Tauri secure storage (browser dev mode excepted)
- **No logging private keys / decrypted content** — vc-crypto constraint
- **No mocking crypto ops** — use real vodozemac in tests
- **No direct main commits** — feature branches only
- **No console.log in prod** — stripped by Vite build config
- **Protocol changes are BREAKING** — ClientEvent/ServerEvent in vc-common

## COMMANDS

```bash
# Development
make setup                # Full dev environment setup
make dev                  # Server with auto-reload (cargo watch)
make client               # Tauri dev (Solid.js + Rust)

# Testing
cargo test -p vc-server   # Server tests (uses cargo-nextest in CI)
bun run test:run          # Frontend tests (vitest, single run)
npx playwright test       # E2E tests

# Quality Gates (run before PR)
SQLX_OFFLINE=true cargo clippy -- -D warnings
cargo fmt --check
cargo deny check licenses
bun run lint && bun run format

# Database
sqlx migrate run --source server/migrations
sqlx migrate add -r <name> --source server/migrations

# Docker
make docker-up            # Start PostgreSQL 16, Valkey, RustFS
make docker-down          # Stop services
```

## NOTES

- **Dual-mode client**: `window.__TAURI__` detection in `lib/tauri.ts` branches native vs browser
- **SQLx offline cache**: CI uses `SQLX_OFFLINE=true` — regenerate `.sqlx/` when changing queries
- **Optional services**: S3 and rate limiting degrade gracefully if unavailable
- **WebRTC ports**: Server exposes 10000-10100/udp for voice RTP
- **Stack**: PostgreSQL 16 + Valkey (Redis-compatible) + optional RustFS (S3)
- **MSRV**: Rust 1.82, Edition 2021
- **Voice**: SFU architecture (not MCU) — server forwards RTP, clients mix audio
- **E2EE**: Text via Olm/Megolm (vodozemac), voice via DTLS-SRTP (server-trusted, MLS planned)
- **Permissions**: Bitflags u64, role stacking with channel overrides, deny > allow
- **Tauri 2.0**: 100+ commands in `client/src-tauri/src/lib.rs`
- **Tech debt**: Tracked in `docs/project/tech-debt.md` (14 open items)
