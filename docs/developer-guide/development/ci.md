# CI Pipeline

The CI pipeline runs on every push to `main`/`develop` and on pull requests to `main`. It is defined in `.github/workflows/ci.yml`.

## Jobs

| Job | Depends On | Description |
|-----|-----------|-------------|
| **Rust Lint (fmt)** | -- | Checks formatting with `cargo +nightly fmt` |
| **Rust Lint (clippy)** | -- | Runs clippy with `-D warnings` |
| **Rust Tests** | -- | Runs all tests including `@everyone` security test |
| **Frontend** | -- | TypeScript type check and Vite build |
| **License Compliance** | -- | `cargo deny check licenses` and advisories |
| **Secrets Scan** | -- | Gitleaks scan for leaked credentials |
| **Docker Build** | rust-lint, rust-test, licenses | Builds the server Docker image |
| **Tauri** (3 platforms) | rust-lint, rust-test, frontend, licenses | Builds desktop client for Linux, macOS, Windows |

### Rust Tests

The `rust-test` job runs with PostgreSQL and Valkey service containers. It includes two test steps:

1. **Regular tests** -- `cargo nextest run --all-features --workspace --exclude vc-client -j 1`
2. **Ignored security tests** -- Runs the `@everyone` permission security test specifically:
   ```bash
   cargo nextest run --all-features -p vc-server \
     -E 'test(test_cannot_grant_dangerous_permissions_to_everyone)' \
     --run-ignored ignored-only
   ```

> **Warning:** Do not use `--run-ignored ignored-only` without a filter expression (`-E`). Many ignored tests across the workspace will fail due to database state conflicts from the first test run.

### Docker Build

The Docker image is defined in `infra/docker/Dockerfile`. It uses a multi-stage build:

- **Builder stage** (`rust:1.88-bookworm`) -- Compiles the server binary with dependency caching via dummy source files
- **Runtime stage** (`bitnami/minideb:bookworm`) -- Minimal image with just the binary, migrations, and runtime deps

The workspace has two shared crates (`shared/vc-common`, `shared/vc-crypto`), and the Dockerfile copies both manifests for the dependency caching layer.

### Tauri Builds

Platform-specific notes:

| Platform | Status | Notes |
|----------|--------|-------|
| Ubuntu | Requires `libvpx-dev`, `libpipewire-0.3-dev`, and other system deps | |
| macOS | Requires `libvpx` via Homebrew | |
| Windows | Uses `vcpkg` + explicit `VPX_*` env vars | `vpx.lib` is copied to `libvpx.lib` for `env-libvpx-sys` compatibility |

Icons in `client/src-tauri/icons/` must be committed to the repo. The Tauri bundler needs `icon.ico` for deb packaging and `icon.icns` for macOS.

### Native VP8 Decode

The Tauri client decodes incoming VP8 RTP streams natively via `libvpx`.

- **Dependency:** `env-libvpx-sys = "4"` is a direct dep of the Tauri client crate. It matches the transitive pin from `vpx-encode 0.3.0` (resolves to `env-libvpx-sys 4.0.13`), so there is no dual-version conflict at link time.
- **Windows:** the existing vcpkg `libvpx → vpx.lib` rename workaround in `.github/workflows/tauri-build.yml` (see table above) continues to apply. No new CI changes needed.
- **Integration test:** the fixture-based decode test is gated behind a feature flag and ignored by default. Run it explicitly with:
  ```bash
  cd client/src-tauri
  cargo test --features decode-integration --test vp8_decode
  ```
- **Fixture regeneration:** re-capture `tests/fixtures/vp8_sample.rtp` after any VP8 encoder upgrade and commit the updated `.rtp`:
  ```bash
  cd client/src-tauri
  cargo run --example capture_vp8_fixture
  ```

## Separate Tauri Build Workflow

A dedicated Tauri build workflow exists at `.github/workflows/tauri-build.yml`. It runs independently from the CI pipeline and includes:

- All 4 platform targets (Linux, Windows, macOS Intel, macOS ARM)
- Artifact uploads for each platform
- Release creation on version tags (`v*`)

## Running CI Checks Locally

```bash
# Formatting
cargo +nightly fmt --all -- --check

# Clippy
SQLX_OFFLINE=true cargo clippy --all-features --workspace --exclude vc-client -- -D warnings

# Tests (requires PostgreSQL and Valkey running)
cargo nextest run --all-features --workspace --exclude vc-client -j 1

# Ignored security test
cargo nextest run --all-features -p vc-server \
  -E 'test(test_cannot_grant_dangerous_permissions_to_everyone)' \
  --run-ignored ignored-only

# License check
cargo deny check licenses

# Frontend
cd client && bunx tsc --noEmit && bun run build
```

## CI Guardrails

Use the incident-driven guardrails in `docs/developer-guide/development/ci-guardrails.md`.

Automated check:

```bash
python3 scripts/check_ci_guardrails.py
```
