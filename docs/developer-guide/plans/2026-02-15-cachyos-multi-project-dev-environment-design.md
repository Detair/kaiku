# CachyOS Multi-Project Dev Environment Design

**Date:** 2026-02-15
**Status:** Approved
**Target platform:** CachyOS (Arch Linux based)

## Goal

Create a repeatable development environment pattern that prevents cross-project contamination by default, while remaining fast for daily Rust/Bun/Tauri development.

## Scope

This design defines:
- host-level baseline setup on CachyOS
- per-project isolation model
- standard Makefile target contract used in every repository
- service and port isolation strategy
- operational rules and troubleshooting expectations

This design does not define project-specific application architecture.

## Recommended Approach

Use one containerized dev environment per project (Distrobox), with a shared host runtime (Podman or Docker), and enforce a standard Makefile interface for all repos.

### Why this approach

- strongest isolation without heavy VM overhead
- deterministic onboarding for new devices
- easy project reset/rebuild without affecting others
- single command surface (`make ...`) across repositories

## Alternatives Considered

### 1) Host tools + direnv only

Pros:
- lowest setup complexity
- fast local execution

Cons:
- higher risk of toolchain drift and version conflicts
- greater chance of accidental cross-project cache/env leakage

### 2) Hybrid (host language toolchains, containerized services)

Pros:
- good performance
- less container management

Cons:
- still allows host-level contamination between projects
- harder to guarantee reproducibility

### 3) Per-project containers (selected)

Pros:
- clean project boundaries
- reproducible across machines
- explicit lifecycle per repository

Cons:
- slightly higher initial setup time

## Architecture

### Layer 1: Host (shared and minimal)

Install only:
- container runtime (`podman` preferred on Arch/CachyOS, `docker` acceptable)
- `distrobox`
- `git`
- editor/IDE
- optional `direnv`

Do not rely on global Rust/Bun/Node/sqlx for day-to-day project work.

### Layer 2: Project container (isolated)

Each repo gets a dedicated container, naming convention:
- `dev-<org>-<repo>`
- example: `dev-detair-canis`

Inside container:
- Rust toolchain (stable)
- Bun
- Node.js (Playwright compatibility)
- `sqlx-cli`
- native build dependencies required by project (GTK/WebKit/libsoup/clang/audio libs for Tauri projects)

### Layer 3: Project services (isolated)

Every repository starts services via compose with a unique project name:
- `COMPOSE_PROJECT_NAME=<repo>`

Where required, use repo-specific port overrides to avoid collisions when multiple projects are running simultaneously.

## Standard Repository Contract (Makefile-first)

Every repository should expose the same baseline targets:

- `make init` - create or refresh the project Distrobox container
- `make bootstrap` - install project dependencies inside the container
- `make doctor` - verify toolchain, env, services, and port availability
- `make services-up` - start project-local compose services
- `make services-down` - stop project-local compose services
- `make migrate` - run database migrations inside container
- `make dev` - run project in development mode
- `make test` - execute test suite inside container
- `make lint` - run lints/static checks inside container
- `make fmt` - run formatters inside container
- `make clean` - remove project-local artifacts only

Optional targets (recommended when applicable):
- `make logs`
- `make shell`
- `make rebuild`

## Data and State Isolation Rules

- one repo = one container
- one repo = one `.env`
- never share `.venv`, `node_modules`, `target`, caches, or DB volumes across repositories
- always execute build/test/migration commands from inside the project container
- avoid global scripts that mutate sibling repositories

## Operational Workflow

1. Clone repository to standard workspace path (for example `~/dev/<org>/<repo>`).
2. Run `make init`.
3. Run `make bootstrap`.
4. Run `make services-up`.
5. Run `make doctor`.
6. Start development with `make dev`.

## Error Handling and Recovery

- If container dependency drift occurs, run `make rebuild` or remove/recreate that project's container only.
- If service conflicts occur, confirm unique compose project names and port overrides.
- If migrations fail, rerun from container shell and verify `.env` for current repo.
- If tool checks fail, `make doctor` must report exact missing component and remediation hint.

## Testing and Verification Expectations

For each project, `make test`, `make lint`, and `make fmt` should be deterministic in a fresh container.

For this repository specifically, the setup must support existing quality gates:
- `cargo test`
- `bun run test:run`
- `cargo fmt --check && cargo clippy -- -D warnings`

## Security and Compliance Notes

- Keep secrets in repo-local `.env` only; never in global shell profiles.
- Prefer least-privilege container execution and avoid privileged containers.
- Keep dependency and storage choices license-compliant with project constraints.

## Deliverable Plan

Create a reusable Markdown manual that can be copied to other repositories and used as the standard onboarding and operations guide for this setup pattern.
