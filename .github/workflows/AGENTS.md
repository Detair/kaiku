<!-- Parent: ../../AGENTS.md -->
# GitHub Actions Workflows

## Purpose

CI/CD pipeline definitions for the Kaiku project. Handles automated testing, building, security scanning, and releases.

## Key Files

| File | Purpose |
|------|---------|
| `ci.yml` | Main CI pipeline: test, lint, build on every push/PR |
| `release.yml` | Release automation: builds, packages, and publishes releases |
| `security.yml` | Security scanning: dependency audits, SAST analysis |
| `tauri-build.yml` | Tauri client builds for all platforms |

### Documentation Files

| File | Purpose |
|------|---------|
| `PRIORITY_2_IMPROVEMENTS.md` | Planned CI/CD improvements |
| `RELEASE_WORKFLOW_FIX.md` | Historical; points to the release runbook |

Release process, maintainer setup and troubleshooting: `docs/ops/release-runbook.md`.

## For AI Agents

### Workflow Triggers

**`ci.yml`** - push to `main` and pull requests.

**`release.yml`** - runs on:
- Tag push `v*.*.*`: publish run (creates a draft release).
- Manual `workflow_dispatch` with inputs `tag` and `dry_run` (default `true`).
  `dry_run=true` builds unsigned and publishes nothing. `dry_run=false`
  publishes, and only passes the tag check when dispatched with `--ref <tag>`.

**`security.yml`** - schedule (weekly), push, manual trigger.

**`tauri-build.yml`** - push to `main`/`develop`, pull requests to `main`,
manual trigger. CI-only desktop build; it never publishes.

**`release-notes-on-merge.yml`** - pull request merged into `main`.

Shared setup: `.github/actions/setup-native-deps` installs the native Tauri
dependencies for `ci.yml`, `tauri-build.yml` and `release.yml`.

### CI Pipeline (`ci.yml`)

Jobs: `rust-lint`, `docs-governance`, `rust-test`, `licenses`, `secrets`,
`observability-contract`, `frontend`, `docker`, `tauri`, `android`.

### Release Pipeline (`release.yml`)

1. `prepare-release` - parse the tag (`scripts/release_version.py parse`),
   decide publish vs dry run, on publish verify version files and that the tag
   is the checked-out commit, generate release notes.
2. `build-tauri` - Linux (AppImage, deb), Windows (NSIS), macOS x86_64 and
   aarch64 (app, dmg). Updater signing; optional macOS/Windows signing hooks.
3. `build-docker` - build, smoke-test and export the server image as the
   `server-image` artifact. Never pushes.
4. `build-android` - signed `Kaiku-<ver>.apk`, verified by
   `scripts/android_verify_apk.sh`.
5. `publish-release` - publish runs only: collect assets, generate
   `latest.json` (all 4 platforms or fail), verify every updater signature
   against `plugins.updater.pubkey` (`scripts/verify_updater_signatures.py`),
   create the draft GitHub release.
6. `push-image` - publish runs only, after all of the above: push the
   smoke-tested image as `ghcr.io/detair/kaiku-server:<ver>` (`latest` only
   for finals).

Runs are serialised per tag (`concurrency`, never cancelled).

Secret gating: on a publish run a missing secret is an `::error::` and the run
fails; on a dry run it is a `::notice::` and the build continues unsigned
(`scripts/require_secrets.py`). Never print secret values; never create
secrets, Environments, tags on `main` or published releases as an agent.

### Security Pipeline (`security.yml`)

Jobs: `rust-audit`, `dependencies`, `bun-audit`, `secrets-scan`, `codeql`,
`osv-scan`.

### Adding New Workflows

```yaml
name: New Workflow

on:
  push:
    branches: [main]
  pull_request:

jobs:
  job-name:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8 # master
      - name: Run task
        run: cargo test
```

### Secrets and Variables

Secrets live in the GitHub Environment `release` (publish runs only).

| Name | Kind | Purpose |
|------|------|---------|
| `GITHUB_TOKEN` | automatic | Release and GHCR push |
| `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` | secrets | Android APK signing (required to publish) |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secrets | Updater bundle signing (required to publish; empty password = no password) |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | secrets | macOS signing, only if `MACOS_SIGNING_ENABLED` is `true` |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | secrets | Windows signing, only if `WINDOWS_SIGNING_ENABLED` is `true` |
| `MACOS_SIGNING_ENABLED`, `WINDOWS_SIGNING_ENABLED` | variables | Switch the signing hooks on |
| `AZURE_TRUSTED_SIGNING_ENDPOINT`, `AZURE_TRUSTED_SIGNING_ACCOUNT`, `AZURE_TRUSTED_SIGNING_PROFILE` | variables | Windows Trusted Signing target |

### Common Issues

**Build failures:**
- Check `Cargo.lock` is committed
- Verify `sqlx` offline mode data is current
- Check for platform-specific issues in matrix builds

**Release failures:**
- Version files out of sync with the tag: `python3 scripts/release_version.py check --tag <tag>`
- Missing signing secret: the log names it; see `docs/ops/release-runbook.md`
- Validate artifact paths

### Local Testing

```bash
# Install act for local workflow testing
brew install act  # macOS

# Run CI workflow locally
act push -j lint

# Run with secrets
act push --secret-file .secrets
```

## Dependencies

- GitHub Actions runners (ubuntu-latest, macos-latest, windows-latest)
- Docker for containerized builds
- Platform-specific toolchains for Tauri builds
