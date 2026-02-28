# Kaiku Platform — Claude Code Projektkontext

Self-hosted Voice/Text-Chat für Gaming-Communities. Rust (Server + Tauri Client), Solid.js (Frontend), PostgreSQL, Valkey.
**Lizenz:** MIT OR Apache-2.0 (Dual License)

## Hard Constraints

### Lizenz-Compliance (KRITISCH)
```bash
cargo deny check licenses
# VERBOTEN: GPL, AGPL, LGPL (static linking)
# ERLAUBT: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0
```

### Performance-Ziele
- Voice-Latenz: <50ms Ende-zu-Ende
- Client RAM (Idle): <80MB
- Client CPU (Idle): <1%
- Startup: <3s

### Security-Basics
- TLS 1.3 für alle Verbindungen
- Passwörter: Argon2id
- JWT: 15min Gültigkeit, EdDSA oder RS256
- Input-Validierung: Immer server-side

## Code-Stil

### Rust
```rust
// Error Handling: thiserror für Library, anyhow für Application
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Channel nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Keine Berechtigung")]
    Forbidden,
}

// Async: tokio mit tracing
#[tracing::instrument(skip(pool))]
async fn get_channel(pool: &PgPool, id: Uuid) -> Result<Channel, ChannelError> {
    // ...
}
```

### TypeScript/Solid.js
```typescript
// Signals für reaktiven State
const [messages, setMessages] = createSignal<Message[]>([]);

// Tauri Commands typsicher aufrufen
import { invoke } from '@tauri-apps/api/core';
const channel = await invoke<Channel>('get_channel', { id });
```

## Commit Convention

**Format:** `type(scope): subject`
**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`
**Scopes:** `auth`, `voice`, `chat`, `db`, `api`, `ws`, `ratelimit`, `infra`, `client`, `crypto`
**Rules:** Max 72 chars, imperative mood, breaking changes: `type(scope)!: message`

## Changelog

> Format: [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)

**Jede benutzerrelevante Änderung MUSS in `CHANGELOG.md` unter `[Unreleased]` dokumentiert werden.**
- `### Added` / `### Changed` / `### Deprecated` / `### Removed` / `### Fixed` / `### Security`
- Nutzer-Perspektive, konkret, mit Issue-Referenz `(#123)` wenn vorhanden
- **Nicht** aktualisieren bei: reinen Refactorings, interner Reorganisation, Docs-Updates, Test-Änderungen ohne Feature-Bezug

## Git Workflow

> Full specification: `docs/plans/2026-01-18-git-workflow-design.md`

**NEVER commit directly to `main`.** Always create a feature branch or worktree for changes and merge via PR.
**Branch naming:** `feature/<name>`, `fix/<name>`, `refactor/<area>`, `docs/<topic>`
**Worktrees:** Main worktree stays on `main`, one worktree per feature, clean up after merge:
`git worktree remove .claude/worktrees/<name> && git branch -d <branch> && git push origin --delete <branch> && git fetch --prune`
**Merge strategy:** Squash merge via `gh pr merge <number> --squash`

### Pre-Push Quality Gates
1. `cargo test` (server), `bun run test:run` (client, uses vitest)
2. `cargo fmt --check && SQLX_OFFLINE=true cargo clippy -- -D warnings`
3. Self-review: no secrets, correct scope, proper error handling
4. Code review for significant changes (new modules, auth/crypto, API changes)

## Code Reviews

```
Review this PR                                    # Full review (8 concerns)
Review src/api/channels.rs for security only      # Scoped review
Ask [Faramir|Elrond|Gandalf|Éowyn|Pippin] about [topic]  # Character deep-dive
```

See `docs/development/code-review.md` for full review format, concern areas, severity criteria, and character descriptions.

## Quick Reference

### Erlaubte Lizenzen
MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, CC0-1.0, Unlicense, MPL-2.0, Unicode-DFS-2016

### Verbotene Lizenzen
GPL-2.0, GPL-3.0, AGPL-3.0, LGPL-2.0, LGPL-2.1, LGPL-3.0, SSPL, Proprietary

### Build Commands
- Server only: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
- Full workspace: `SQLX_OFFLINE=true cargo clippy -- -D warnings`
- `.sqlx/` contains offline query cache — commit new `.sqlx/*.json` files when queries change

### Wichtige Crates
- Web: axum, tower, tokio
- OpenAPI: utoipa, utoipa-swagger-ui
- WebRTC: webrtc-rs
- DB: sqlx (PostgreSQL)
- Redis: fred
- Auth: jsonwebtoken, argon2, openidconnect
- E2EE Text: vodozemac
- Crypto: rustls, x25519-dalek, ed25519-dalek

### Package Manager
- Bun (for package management and script running)
- Node.js (still required for Playwright tests)

### Wichtige Frontend Packages
- Framework: solid-js
- Build: vite, typescript
- Styling: unocss
- Icons: lucide-solid

## Documentation Pointers

- `PROJECT_SPEC.md` — Anforderungen und Entscheidungslog
- `ARCHITECTURE.md` — Technische Architektur und Diagramme
- `STANDARDS.md` — Verwendete Protokolle und Libraries
- `LICENSE_COMPLIANCE.md` — Lizenzprüfung aller Dependencies
- `CHANGELOG.md` — Änderungsprotokoll
- `docs/development/code-review.md` — Review-System, Concern Areas, Characters, Workflows
