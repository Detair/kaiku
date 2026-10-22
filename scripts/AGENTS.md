<!-- Parent: ../AGENTS.md -->
# Scripts

## Purpose
Build, development, and utility scripts for the project.

## Key Files
- `dev-setup.sh` - Complete development environment setup
- `create-test-users.sh` - Create test users in development
- `check_docs_governance.py` - Validates roadmap/changelog/lifecycle consistency
- `generate_release_notes.py` - Builds standardized milestone release notes
- `release_version.py` - Single source of the release version (parse/bump/check/android-code); CI runs `check`
- `verify_updater_signatures.py` - Publish gate: verifies every `latest.json` signature against `plugins.updater.pubkey` (needs `minisign`)
- `bump-version.sh` - Bumps the version in all version files: `scripts/bump-version.sh X.Y.Z[-rc.N]`
- `tests/` - stdlib `unittest` tests for scripts (`python3 -m unittest discover -s scripts/tests -v`)

## For AI Agents

### Script Usage

#### Initial Setup
```bash
./scripts/dev-setup.sh
# Sets up: Docker services, database migrations, test data
```

#### Create Test Users
```bash
./scripts/create-test-users.sh
# Creates: admin/admin123, alice/password123, bob/password123
```

#### Release Version
The version lives in `client/src-tauri/tauri.conf.json`, root `Cargo.toml`
(`[workspace.package]`), `client/package.json` and
`mobile/android/version.properties` (`versionName` + `versionCode`); the
workspace crates in `Cargo.lock` follow. Never edit these by hand:
```bash
./scripts/bump-version.sh 1.0.0-rc.1   # rewrites only the version lines
python3 scripts/release_version.py check [--tag v1.0.0-rc.1]
```
Accepted versions: `X.Y.Z` or `X.Y.Z-rc.N` (each part 0-99, rc 1-98, optional
leading `v`). Android `versionCode = major*1000000 + minor*10000 + patch*100 + (rc or 99)`.

### Script Standards
- All scripts should be POSIX-compatible where possible
- Use `set -e` for error handling
- Include usage/help text
- Document prerequisites at the top
- Keep scripts deterministic and CI-safe

### When to Add Scripts
- Automating multi-step processes
- Development environment tasks
- CI/CD helpers
- One-time setup tasks

Avoid scripts for:
- Simple single commands (use Makefile instead)
- Platform-specific tasks (document alternatives)
