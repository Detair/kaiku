# Kaiku

Self-hosted voice and text chat for gaming communities. Rust server (axum, sqlx), Tauri 2 desktop client with a Solid.js frontend, Android client under `mobile/android/`, PostgreSQL and Valkey. Dual licensed MIT OR Apache-2.0.

Structure and "where to look" tables live in `AGENTS.md` at the root and in each directory. Read the `AGENTS.md` of a directory before changing code in it. This file holds the rules that apply everywhere.

## Constraints

### Licenses

Kaiku is distributed as a binary, so a copyleft dependency would force its terms on the whole product. Run `cargo deny check licenses` after adding or updating a dependency.

- Allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, CC0-1.0, Unlicense, MPL-2.0, Unicode-DFS-2016.
- Not allowed: GPL, AGPL, LGPL (static linking), SSPL, proprietary.
- `LICENSE_COMPLIANCE.md` records the check for every dependency.

### Performance targets

Voice latency under 50 ms end to end. Client idle: under 80 MB RAM and under 1 % CPU. Startup under 3 s.

### Security

- TLS 1.3 for every connection. Passwords with Argon2id. JWT valid for 15 minutes, EdDSA or RS256.
- Validate input on the server, whatever the client already checked.
- End-to-end encryption uses vodozemac only. No custom cryptography.
- Changes under `server/src/auth/` and to crypto code need a security review before merge.

### UI contrast

Accent colours are for decoration and indicators. Text that must be readable uses the text tokens, because accent on accent has too little contrast.

- Readable text: `text-text-primary`. Selected or active state: `text-text-primary` on `bg-accent-primary/20` or stronger, never `text-accent-primary` on `bg-accent-primary/*`.
- No `opacity-*` below 50 % on text the user needs to read. `text-text-secondary` is the weakest readable tier.
- On dark overlays use `text-text-muted`, not `text-white/40` or lower.
- Targets (WCAG): `text-primary` at least 7:1 on `surface-base`, `text-secondary` at least 4.5:1 on `surface-layer1`, `text-muted` at least 3:1.

## Conventions

- **Wire format is `snake_case`.** TypeScript interface fields that mirror WebSocket, API or JSONB data use `snake_case` to match Rust serde defaults. Local variables and parameters stay camelCase. See `docs/developer-guide/development/standards.md` §5.
- **Rust errors:** `thiserror` in library code, `anyhow` at the application edge. Async code runs on tokio with `tracing`.
- **Package manager is Bun.** Node.js is only needed for the Playwright tests.
- **Android strings** mirror the web client's keys; rules in `mobile/android/AGENTS.md`. `bun run i18n:check` in `client/` enforces them.
- **Module layout** (Tier 1 and Tier 2, error/types/queries files, when to split handlers): `docs/superpowers/specs/2026-04-10-codebase-consistency-standards-design.md`.

## Commands

```bash
# Server and workspace
cargo test
cargo fmt --check
SQLX_OFFLINE=true cargo clippy -- -D warnings                 # whole workspace
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings    # server only

# Client (in client/)
bun run test:run
bun run lint
bun run i18n:check
```

`.sqlx/` holds the offline query cache. When a query changes, regenerate it and commit the new `.sqlx/*.json` files, otherwise the offline build fails. `make help` lists the development targets (setup, dev server, client).

## Git workflow

Full specification: `docs/developer-guide/plans/2026-01-18-git-workflow-design.md`.

- Do not commit to `main`. Work on a branch in a worktree under `.claude/worktrees/<name>` and merge through a pull request. The main worktree stays on `main`.
- Branch names: `feature/<name>`, `fix/<name>`, `refactor/<area>`, `docs/<topic>`.
- Merge with squash: `gh pr merge <number> --squash`. Afterwards remove the worktree and the branch, locally and on the remote.
- Before pushing: the tests and checks above pass, and you have read your own diff for secrets, scope and error handling. New modules, auth, crypto and API changes get a code review.

### Commits

`type(scope): subject`, at most 72 characters, imperative mood. Breaking change: `type(scope)!: subject`.

- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`.
- Scopes: `auth`, `voice`, `chat`, `db`, `api`, `ws`, `ratelimit`, `infra`, `client`, `crypto`.

### Changelog

Every change a user can notice goes into `CHANGELOG.md` under `[Unreleased]` ([Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)): `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or `Security`, written from the user's point of view, with the issue reference `(#123)` if there is one. Pure refactorings, internal reorganisation, documentation and test-only changes stay out.

## Code reviews

`docs/developer-guide/development/code-review.md` defines the review format, the eight concern areas, the severity criteria and the reviewer characters (Faramir, Elrond, Gandalf, Éowyn, Pippin). "Review this PR" means the full review; a request can be scoped to a file or to one concern.

## Documentation

- `docs/developer-guide/project/specification.md` – requirements and decision log.
- `docs/developer-guide/architecture/overview.md` – architecture and diagrams.
- `docs/developer-guide/development/standards.md` – protocols and libraries in use.
- `docs/developer-guide/development/i18n.md` – translation rules for web and Android.
