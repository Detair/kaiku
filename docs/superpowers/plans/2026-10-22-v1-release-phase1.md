# v1.0 Family Release — Release & CI (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pushed `vX.Y.Z[-rc.N]` tag produces, via `release.yml` alone, a draft GitHub release with signed desktop installers for Linux/Windows/macOS (Intel + Apple Silicon), a valid `latest.json`, a signed upgradable `Kaiku-<ver>.apk`, and `ghcr.io/detair/kaiku-server:<ver>`. Every version string in the repo agrees with the tag.

**Architecture:** Python owns the logic: version parsing and stamping, secret gating, and the updater manifest. That logic sits under `scripts/` and is unit-tested with stdlib `unittest`. Workflows call it and contain no logic of their own. A composite action `.github/actions/setup-native-deps` is the single place for per-OS native build dependencies, and `ci.yml`, `tauri-build.yml` and `release.yml` all use it. `scripts/check_ci_guardrails.py` enforces the workflow invariants structurally (PyYAML) so they cannot drift. Signing material reaches only `release.yml` jobs in publish mode, through the GitHub Environment `release`. Dry runs and PR builds receive no signing secret.

**Tech Stack:** GitHub Actions, Python 3 (stdlib `unittest`, PyYAML 6), bash, Tauri 2 CLI, Gradle/AGP 8 (Kotlin DSL), Android build-tools (`apksigner`, `aapt2`), Docker Buildx/GHCR.

**Spec:** `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md` (scope item 4 "Installable Android app", item 7 "Release mechanics"). Review: `docs/reviews/2026-10-22-v1-family-release-readiness.md` (R-01, R-02, R-03; R-06, R-07, R-12, R-14, R-15, R-19, A-16 folded in). Contracts: `docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C7, C8; C6 is not consumed here).

## Global Constraints

- Repo is **public**: no secret values, no keystore, no certificate in any commit, log line or artifact. `::add-mask::` any derived secret before use.
- **Infrastructure is the maintainer's** (spec "Answers & decisions": "infrastructure changes (VPS, DNS, Firebase project, signing certificates, CI secrets) must be cleared with the maintainer first"). Agents never create secrets, Environments, repo variables, keystores, tags on `main`, or published releases. Every task must merge and keep CI green **with all maintainer actions (M1–M7 below) not yet done**.
- Secret gating rule: **publish mode** (tag push, or dispatch with `dry_run=false` on a tag ref) → missing required secret = `::error::` + exit 1. **Dry-run / PR mode** → `::notice::` naming the skipped step, build continues unsigned.
- Versioning is contract **C7** verbatim. Tag `vX.Y.Z[-rc.N]`. `versionCode = MAJOR*1_000_000 + MINOR*10_000 + PATCH*100 + rc`, with rc 1–98 and 99 for a final release. The version string without `v` (e.g. `1.0.0-rc.1`) is called `<ver>` below.
- Server image is contract **C8**: `ghcr.io/detair/kaiku-server:<ver>` (no leading `v`), amd64, built from `infra/docker/Dockerfile`, env-var-only configuration.
- No local Android SDK, JDK or Docker on the agent host. Gradle and Docker behaviour is verified by the PR's CI run, which must be green and is linked in the PR. Python tests and guardrails run locally.
- `bun run tauri build` gets **no `--` separator** (bun passes args through. `release.yml:136` currently passes `--`).
- Branch per task: `fix/<name>` or `feature/<name>` per CLAUDE.md. Squash-merge. Commit format `type(infra): subject` (≤72 chars).
- CHANGELOG `[Unreleased]` entry only where users see a change (Tasks 3 and 6).
- Do not touch `infra/scripts/backup.sh`, `upgrade-preflight.sh` or `docs/ops/backup-restore-runbook.md`. The server plan fixes psql auth (B-06/R-04), and the the production server plan owns backup and restore.
- Review gate for every task: Fable 5.1 plus one external model (Gemini 4 Argon via `agy`, or Codex), per `docs/developer-guide/agents/multi-model-workflow.md`.
- Local verification baseline for every task: `python3 -m unittest discover -s scripts/tests -v` and `python3 scripts/check_ci_guardrails.py` both pass. Each workflow file must load with `python3 -c "import yaml,sys;[yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/*.yml .github/actions/*/action.yml`.

## Maintainer actions (infrastructure, NOT done by agents)

Every task below merges without these. The runbook (Task 7) documents each one step by step.

| ID | Action | Unblocks |
|---|---|---|
| M1 | Create GitHub Environment `release`. Deployment rule: tags `v*` plus branch `main`. Move `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` into it and delete the repo-level copies. | Signed updater artifacts. The key leaves PR builds for good. |
| M2 | Generate the Android release keystore offline and keep 2 off-host backups. Add the 4 Environment secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. | Signed APK on tag builds |
| M3 | After the first image push, set GHCR package `kaiku-server` visibility (public, or a pull token for the production server) | production deployment |
| M4 | (Optional, cost decision) Apple Developer Program. Add `APPLE_*` secrets and set repo variable `MACOS_SIGNING_ENABLED=true`. | Gatekeeper-clean macOS |
| M5 | (Optional, cost decision) Windows signing via Azure Trusted Signing. Add `AZURE_*` secrets and variables, and set `WINDOWS_SIGNING_ENABLED=true`. | SmartScreen-clean Windows |
| M6 | Rehearsal: push throwaway tag `v0.9.0-rc.1` from an unmerged branch `release/rehearsal`. Inspect the draft, then delete the draft, the tag and the image tag. | Proves the publish path before `v1.0.0-rc.1` |
| M7 | Decide how pre-release updater manifests are hosted (R-07): mark the RC as "latest" release, or serve `latest.json` elsewhere | RC "older build offers update" check |

## Review Focus

1. **Tag pushed without a version bump.** For example, `v1.0.0-rc.1` is tagged while the files still say `0.1.0`. Expected: `prepare-release` fails before any build job starts, and the error lists every disagreeing file. Pinned in Task 1 (`test_check_tag_mismatch_lists_each_file`) and Task 3 (`build-*` jobs `needs: prepare-release`, guardrail `test_release_builds_need_prepare`).
2. **versionCode ordering across rc, final and the rehearsal.** An APK from `v0.9.0-rc.1`, if installed, must not block `v1.0.0-rc.1`. The code must rise from rc.98 to final to the next patch's rc.1, and `MINOR`/`PATCH` above 99 must be rejected, not wrapped. Pinned in Task 1 (`test_android_code_ordering`, `test_parse_rejects_out_of_range`).
3. **Signing secret missing.** A tag build must fail loudly. A dry run or PR build must say what it skipped and still finish. A PR from a fork must never see the updater key. Pinned in Task 3 (`RequireSecretsTest`) and Task 2 (`test_pr_workflows_never_reference_signing_key`).
4. **Partial platform failure.** If one OS build fails or produces no `.sig`, `latest.json` must not be published with three of the four platforms. A silently missing platform means those users never update. Pinned in Task 3 (`test_publish_mode_requires_all_platforms`).
5. **Wrong or unsigned APK uploaded.** Examples: the debug APK, an `app-release-unsigned.apk`, or a stale `versionCode`. Publish must refuse it. Pinned in Task 5 (`android_verify_apk.sh` negative case in CI) and Task 6 (verify step uses the C7 code).

---

### Task 1: Version source of truth and stamping (C7, R-02)

Branch `fix/release-versioning`.

**Files:**
- Create: `scripts/release_version.py`, `scripts/bump-version.sh` (executable), `scripts/tests/__init__.py`, `scripts/tests/test_release_version.py`, `mobile/android/version.properties`
- Modify: `mobile/android/app/build.gradle.kts:20-21` (read `version.properties`), `.github/workflows/ci.yml` (`docs-governance` job), `scripts/AGENTS.md`

**Interfaces:**
- Produces (used by Tasks 3, 5 and 6):
  - `@dataclass(frozen=True) class ReleaseVersion: major: int; minor: int; patch: int; rc: int | None`. It has the properties `semver -> str` (`"1.0.0-rc.1"`), `tag -> str` (`"v1.0.0-rc.1"`), `is_prerelease -> bool` and `android_version_code -> int`.
  - `class VersionError(ValueError)`
  - `parse_version(text: str) -> ReleaseVersion`. Accepts with or without a leading `v` and rejects everything else.
  - `VERSION_FILES: dict[str, str]`. It maps the labels `"client/src-tauri/tauri.conf.json"`, `"Cargo.toml"`, `"client/package.json"`, `"mobile/android/version.properties:versionName"` and `"mobile/android/version.properties:versionCode"` to how each is read.
  - `read_versions(root: Path) -> dict[str, str]`
  - `write_version(root: Path, v: ReleaseVersion) -> list[Path]`. It rewrites only the version lines. It also rewrites the `version = "…"` line after `name = "<crate>"` in `Cargo.lock` for `WORKSPACE_CRATES = ("vc-server", "vc-client", "vc-common", "vc-crypto", "kaiku-vp8-decoder")`.
  - `check_consistency(root: Path, expected: ReleaseVersion | None) -> list[str]` returns error strings, each naming a file.
  - CLI: `release_version.py parse <tag>` prints `KEY=value` lines (`version`, `tag`, `is_prerelease`, `android_version_code`) suitable for `>> $GITHUB_OUTPUT`. Other subcommands: `bump <ver>`, `check [--tag <tag>]`, `android-code <ver>`. Exit codes: 0 ok, 1 inconsistent, 2 invalid version or usage.
  - `scripts/bump-version.sh X.Y.Z[-rc.N]` uses `set -eu` and execs `python3 scripts/release_version.py bump "$1"`. Then it prints the next steps: commit `chore(infra): bump version to <ver>`, open a PR, and tag after merge (see the runbook).
  - `mobile/android/version.properties` contains `versionName=0.1.0` and `versionCode=10099`. Gradle reads both and fails configuration with `GradleException("mobile/android/version.properties missing versionName/versionCode")` if either is absent.

- [ ] **Step 1: Write failing tests** in `scripts/tests/test_release_version.py` (class `ReleaseVersionTest`):
```python
def test_parse_final(self):        v = parse_version("v1.0.0"); assert (v.major, v.minor, v.patch, v.rc) == (1, 0, 0, None); assert v.semver == "1.0.0"; assert not v.is_prerelease
def test_parse_rc(self):           v = parse_version("1.0.0-rc.3"); assert v.rc == 3 and v.tag == "v1.0.0-rc.3" and v.is_prerelease
def test_parse_rejects_out_of_range(self):
    for bad in ["1.0", "v1.0.0-beta.1", "v1.0.0-rc.0", "v1.0.0-rc.99", "v1.100.0", "v1.0.100", "v1.0.0-rc.1+meta", " v1.0.0", "v01.0.0"]:
        with self.assertRaises(VersionError): parse_version(bad)
def test_android_code_values(self):
    assert parse_version("1.0.0").android_version_code == 1_000_099
    assert parse_version("1.0.0-rc.1").android_version_code == 1_000_001
    assert parse_version("0.1.0").android_version_code == 10_099
def test_android_code_ordering(self):
    order = ["0.9.0-rc.1", "1.0.0-rc.1", "1.0.0-rc.98", "1.0.0", "1.0.1-rc.1", "1.0.1", "1.1.0-rc.1"]
    codes = [parse_version(s).android_version_code for s in order]; assert codes == sorted(codes) and len(set(codes)) == len(codes)
def test_repo_is_consistent(self):            # guards drift on main
    assert check_consistency(REPO_ROOT, None) == []
def test_bump_rewrites_only_version_lines(self):
    # copy the 5 real files + Cargo.lock into a tmpdir, write_version(tmp, parse_version("1.0.0-rc.1"))
    # assert read_versions(tmp) values are all "1.0.0-rc.1" / "1000001"
    # assert difflib line diff per file: tauri.conf.json, Cargo.toml, package.json → exactly 1 changed line; version.properties → 2; Cargo.lock → 5
def test_check_detects_mismatch(self):        # tmp copy, edit package.json version to 9.9.9
    errs = check_consistency(tmp, None); assert len(errs) == 1 and "client/package.json" in errs[0]
def test_check_tag_mismatch_lists_each_file(self):
    errs = check_consistency(tmp_at_0_1_0, parse_version("v1.0.0-rc.1")); assert len(errs) == 5 and all("expected 1.0.0-rc.1" in e or "expected 1000001" in e for e in errs)
def test_cli_parse_outputs_github_output_lines(self):   # subprocess
    out = run("parse", "v1.0.0-rc.2"); assert out.splitlines() == ["version=1.0.0-rc.2", "tag=v1.0.0-rc.2", "is_prerelease=true", "android_version_code=1000002"]
def test_cli_invalid_exit_2(self): assert run_rc("parse", "v1.0") == 2
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `python3 -m unittest scripts.tests.test_release_version -v`. Expected: `ModuleNotFoundError: release_version`. Import the module with `sys.path.insert(0, scripts_dir)`.

- [ ] **Step 3: Implement `scripts/release_version.py`.**
  - Regex for parsing: `^v?(0|[1-9]\d?)\.(0|[1-9]\d?)\.(0|[1-9]\d?)(?:-rc\.([1-9]\d?))?$`, then reject `rc == 99`.
  - In `tauri.conf.json` and `package.json`, edit only the top-level line `^  "version": "…",$`. Do not round-trip through `json.dump`.
  - In root `Cargo.toml`, edit the `version =` line inside `[workspace.package]`.
  - Then create `version.properties` (versionCode `10099`, the C7 value for 0.1.0) and `bump-version.sh`.

- [ ] **Step 4: Make Gradle read `version.properties`.** In `build.gradle.kts`, load it with `java.util.Properties` from `rootProject.file("version.properties")`. Set `versionCode = props["versionCode"].toString().toInt()` and `versionName = props["versionName"].toString()`.

- [ ] **Step 5: Add the CI gate.** In the `ci.yml` `docs-governance` job, add the steps `python3 -m pip install --user pyyaml==6.0.2`, `python3 -m unittest discover -s scripts/tests -v` and `python3 scripts/release_version.py check`.

- [ ] **Step 6: Run the tests and confirm they pass.** Run `python3 -m unittest discover -s scripts/tests -v` (all pass) and `scripts/bump-version.sh 0.1.0 && git diff --exit-code` (no diff, so the script is idempotent). The PR CI `Android Build` job must be green with `versionCode` 10099.

- [ ] **Step 7: Commit.** `git commit -m "feat(infra): single-source release version with bump script"`.

---

### Task 2: Make `release.yml` buildable through a shared native-deps action (R-01, R-06, R-14 part)

Branch `fix/release-workflow-build`. This task is the first `release.yml` that can build anything. Publishing stays restricted to tag pushes (dry-run semantics arrive in Task 3).

**Files:**
- Create: `.github/actions/setup-native-deps/action.yml`, `client/src-tauri/tauri.ci.conf.json` (`{"bundle":{"createUpdaterArtifacts":false}}`), `scripts/tests/test_check_ci_guardrails.py`
- Modify: `.github/workflows/release.yml` (`build-tauri`, `build-docker`), `.github/workflows/ci.yml` (`tauri` job), `.github/workflows/tauri-build.yml`, `scripts/check_ci_guardrails.py`

**Interfaces:**
- Produces:
  - The composite action `./.github/actions/setup-native-deps` takes no inputs and branches on `runner.os`. Every step sets an explicit `shell:`.
    - **Linux:** `apt-get install` of the `tauri-build.yml:77-90` set plus `patchelf libopus-dev`.
    - **Windows:** `vcpkg install libvpx:x64-windows-static-md` and `choco install -y pkgconfiglite --allow-empty-checksums`. Then a pwsh step writes `VPX_LIB_DIR=`, `VPX_INCLUDE_DIR=`, `VPX_VERSION=1.15.2` and `VPX_STATIC=1` via `Out-File $env:GITHUB_ENV -Append`, as `ci.yml:403-412` does.
    - **macOS and Windows:** set `CMAKE_POLICY_VERSION_MINIMUM=3.5`.
    - **macOS:** `brew install pkg-config libvpx` and write `XCODE_CACHE_KEY` to `$GITHUB_ENV`.
  - Guardrail functions, each taking text or parsed YAML plus `errors: list[str]`:
    - `check_windows_libvpx_guardrails(action_text: str, errors)` now checks the action, not the workflows.
    - `check_native_deps_usage(workflows: dict[str, str], errors)`
    - `check_release_workflow(release: dict, repo_root: Path, errors)`
    - `check_no_signing_key_in_pr_workflows(workflows: dict[str, str], errors)`
  - `release.yml` `build-tauri` matrix has exactly one row per target. Each row carries `os`, `target` and `bundles`:

    | os | target | bundles |
    |---|---|---|
    | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `appimage,deb` |
    | `windows-latest` | `x86_64-pc-windows-msvc` | `nsis` |
    | `macos-15-intel` | `x86_64-apple-darwin` | `app,dmg` |
    | `macos-latest` | `aarch64-apple-darwin` | `app,dmg` |

    The job sets `defaults.run.shell: bash` and builds with `bun run tauri build --target "$BUILD_TARGET" --bundles "$BUNDLES"`.
  - Decision: **MSI is dropped.** WiX rejects non-numeric pre-release versions such as `-rc.1`, and the updater already prefers `*-setup.exe`. If the Task 3 dry run shows NSIS also rejects `-rc.N`, record the result in the PR and stop for a decision.

- [ ] **Step 1: Write failing guardrail tests** in `scripts/tests/test_check_ci_guardrails.py`. Each test feeds a minimal YAML or text fixture into one function and asserts on `errors`:
```python
def test_release_dockerfile_must_exist(self):        # file: infra/docker/server.Dockerfile → error contains "Dockerfile"
def test_release_rejects_intel_target_on_arm_runner(self):  # {os: macos-latest, target: x86_64-apple-darwin} → error contains "macos-15-intel"
def test_release_one_row_per_target(self):           # two rows with same target → error contains "duplicate target"
def test_release_build_shell_is_bash(self):          # build-tauri without defaults.run.shell bash → error
def test_release_builds_need_prepare(self):          # build-tauri/build-docker missing needs: prepare-release → error
def test_native_deps_action_used_everywhere(self):   # ci.yml tauri job lacking "uses: ./.github/actions/setup-native-deps" → error naming ci.yml
def test_libvpx_tokens_checked_in_action(self):      # action text missing "VPX_STATIC=1" → error naming action.yml
def test_pr_workflows_never_reference_signing_key(self):   # "TAURI_SIGNING_PRIVATE_KEY" in ci.yml or tauri-build.yml → error
def test_repo_passes(self):                          # main() on the real repo returns 0
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `python3 -m unittest scripts.tests.test_check_ci_guardrails -v`. Expected: the new functions do not exist yet, and `test_repo_passes` fails on `release.yml`.

- [ ] **Step 3: Extend `check_ci_guardrails.py`.** Load the workflows with `yaml.safe_load`. Note that PyYAML parses `on:` as key `True`. `main()` checks the action plus all three workflows. Keep the existing setup_integration and uploads checks unchanged.

- [ ] **Step 4: Create the composite action** and replace the inline dependency steps in `ci.yml` (`tauri` job) and `tauri-build.yml` with `- uses: ./.github/actions/setup-native-deps`, placed after `actions/checkout`. Leave the `rust-lint` and `rust-test` minimal apt lines unchanged.

- [ ] **Step 5: Remove the updater key from PR workflows.**
  - In `ci.yml` (`tauri`) and `tauri-build.yml`, delete the `TAURI_SIGNING_*` env and build with `bun run tauri build --config src-tauri/tauri.ci.conf.json` (working dir `client`).
  - In `tauri-build.yml`, drop `tags: ['v*']` from `on.push` and delete the `release` job (R-06).
  - Fix the `ci.yml` artifact upload paths to `target/release/bundle/...`. The workspace root owns `target/`.

- [ ] **Step 6: Rewrite the `release.yml` builds.**
  - **`build-tauri`:** use the matrix above, `defaults.run.shell: bash`, `dtolnay/rust-toolchain@stable` with `targets: ${{ matrix.target }}`, then the composite action, then `Swatinem/rust-cache@v2` with `key: ${{ env.XCODE_CACHE_KEY }}`. One bash checksum step replaces the three OS-specific ones (`sha256sum`, falling back to `shasum -a 256` on macOS). The upload artifact is named `tauri-${{ matrix.target }}`.
  - **`build-docker`:** set `file: infra/docker/Dockerfile` and `permissions: {contents: read, packages: write}`.
  - Guard pushing and publishing: `push: ${{ github.ref_type == 'tag' }}`, and on `publish-release` set `if: github.ref_type == 'tag'`. This keeps a dispatch run safe before Task 3.
  - The updater key stays as it is in `build-tauri` for now. Task 3 moves it.

- [ ] **Step 7: Run the tests and confirm they pass.** Run the local baseline (Global Constraints). On the PR, CI `Tauri ubuntu-latest`/`macos-latest` and `tauri-build.yml` must be green on all four OS rows. Windows may still be `allow_failure`, but its log must show the composite action succeeding.

- [ ] **Step 8: Commit.** `git commit -m "fix(infra): make release workflow build via shared native deps action"`.

---

### Task 3: Release pipeline — version gate, dry run, Environment, manifest, signing hooks (R-01 dry-run, R-14, R-15, R-19, R-12 hooks)

Branch `feature/release-pipeline-gates`. Depends on Tasks 1 and 2.

**Files:**
- Create: `scripts/require_secrets.py`, `scripts/generate_updater_manifest.py`, `scripts/tests/test_require_secrets.py`, `scripts/tests/test_generate_updater_manifest.py`, `scripts/tests/test_generate_release_notes.py`
- Modify: `.github/workflows/release.yml`, `scripts/generate_release_notes.py`, `scripts/check_ci_guardrails.py` (+ tests), `CHANGELOG.md`

**Interfaces:**
- Consumes: `release_version.py parse|check|bump` (Task 1); the matrix and composite action (Task 2).
- Produces:
  - `require_secrets.py --mode {publish,dry-run} --output <name> VAR [VAR...]`. It reads the environment. If every variable is non-empty, it writes `<name>=true` to `$GITHUB_OUTPUT` and exits 0. If any are missing in dry-run, it writes `<name>=false`, prints `::notice::<names> not set — <step> skipped (dry run)` and exits 0. If any are missing in publish, it prints `::error::Missing required secrets in Environment 'release': <names>` and exits 1. It never prints values.
  - `generate_updater_manifest.py --assets-dir <dir> --tag <tag> --repo Detair/kaiku --mode {publish,dry-run} --output <file>` writes the `latest.json` schema of `release.yml:310-316`. Platform keys are `linux-x86_64` (`*.AppImage`), `windows-x86_64` (`*-setup.exe`), `darwin-x86_64` (`*x86_64.app.tar.gz`) and `darwin-aarch64` (`*aarch64.app.tar.gz`), each requiring a sibling `.sig`. In publish mode a missing platform exits 1 and names it. In dry-run it warns.
  - `generate_release_notes.py` gains `extract_section(changelog: str, version: str) -> str | None`, which returns the body of `## [<version>]` or `None`. `main()` uses it when the `## [<ver>]` section exists (curated final) and falls back to `[Unreleased]` otherwise.
  - `release.yml` job `prepare-release` outputs `version`, `tag`, `is_prerelease`, `android_version_code`, `publish` (`"true"`/`"false"`) and `release_notes`.
  - `release.yml` gets a `workflow_dispatch` input `dry_run: boolean, default true`.
  - Repo variables `MACOS_SIGNING_ENABLED` and `WINDOWS_SIGNING_ENABLED` are read as `vars.X == 'true'`. Unset means disabled.

- [ ] **Step 1: Write failing tests.**
```python
# test_require_secrets.py — subprocess with controlled env, GITHUB_OUTPUT=tmpfile
def test_publish_missing_fails_and_names_var(self):   rc==1; "ANDROID_KEY_ALIAS" in stdout; secret values of present vars not in stdout
def test_dry_run_missing_notices_and_continues(self): rc==0; output file contains "signing=false"; "::notice::" in stdout
def test_all_present_true(self):                      rc==0; output "signing=true"
def test_empty_string_counts_as_missing(self):        VAR="" → treated as missing
# test_generate_updater_manifest.py — tmp assets dir with fake files + .sig
def test_all_platforms_manifest(self):     4 keys; url == "https://github.com/Detair/kaiku/releases/download/v1.0.0-rc.1/Kaiku_1.0.0-rc.1_amd64.AppImage"; version == "1.0.0-rc.1"
def test_publish_mode_requires_all_platforms(self):   drop darwin-x86_64 .sig → rc==1, "darwin-x86_64" in stderr
def test_dry_run_tolerates_missing(self):  rc==0 with 0 platforms, no file written
# test_generate_release_notes.py
def test_extract_section_returns_curated_block(self): changelog with "## [1.0.0] - 2026-12-10\n### Added\n- x\n## [0.1.0]" → returns block containing "- x", not "0.1.0"
def test_extract_section_missing_returns_none(self)
# test_check_ci_guardrails.py additions
def test_release_signing_env_only_in_publish(self):  a job referencing secrets.TAURI_SIGNING_PRIVATE_KEY must have environment expression containing "release" and gate on publish → else error
def test_release_declares_permissions(self):         top-level permissions missing → error
def test_release_has_no_docs_governance_step(self):  "check_docs_governance.py" in release.yml → error
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `python3 -m unittest discover -s scripts/tests -v`. Expected: the new modules are missing and the guardrail additions fail on the current `release.yml`.

- [ ] **Step 3: Implement the three scripts** to the interfaces above, then add the guardrail functions `check_release_secret_scoping(release: dict, errors)` and `check_release_hygiene(release_text: str, release: dict, errors)`.

- [ ] **Step 4: Rewrite `release.yml` job by job.**
  - **Top level:** set `permissions: {contents: read}`. Grant `contents: write` and `packages: write` only to the jobs that need them.
  - **`prepare-release`:** `release_version.py parse "$TAG" >> "$GITHUB_OUTPUT"` (exit 2 fails the run). Compute `publish = event == push && ref_type == tag || (dispatch && !dry_run)`.
    - When `publish` is true: run `release_version.py check --tag "$TAG"` (must exit 0) and assert `git rev-parse "$TAG^{commit}" == HEAD`.
    - Remove `check_docs_governance.py`.
  - **`build-tauri`:**
    - Set `environment: ${{ needs.prepare-release.outputs.publish == 'true' && 'release' || '' }}`.
    - Dry run only: a step `scripts/bump-version.sh "$VERSION"` stamps the workspace so the artifacts report the dry-run version.
    - Run `require_secrets.py --output updater TAURI_SIGNING_PRIVATE_KEY`, with the env set from `secrets.*` only when `publish == 'true'`.
    - If `updater == 'false'`, build with `--config src-tauri/tauri.ci.conf.json`.
  - **macOS signing hook:**
    - If `vars.MACOS_SIGNING_ENABLED == 'true'`: `require_secrets.py --mode <mode> --output macsign APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID`, and pass those as env to the build (Tauri reads them natively).
    - Otherwise: set `APPLE_SIGNING_IDENTITY=-` (ad-hoc, required for Apple Silicon to launch) and print `::notice::macOS build is ad-hoc signed; Gatekeeper will warn (set MACOS_SIGNING_ENABLED, see docs/ops/release-runbook.md)`.
  - **Windows signing hook:**
    - If `vars.WINDOWS_SIGNING_ENABLED == 'true'`: require `AZURE_CLIENT_ID AZURE_CLIENT_SECRET AZURE_TENANT_ID` and the vars `AZURE_TRUSTED_SIGNING_ENDPOINT/ACCOUNT/PROFILE`. Then `cargo install trusted-signing-cli --locked --version <pin, latest at implementation time, recorded in PR>`. Write `$RUNNER_TEMP/tauri-winsign.json` with `bundle.windows.signCommand` = `trusted-signing-cli -e <endpoint> -a <account> -c <profile> -d Kaiku %1` and add `--config` for it.
    - Otherwise: print `::notice::Windows installer is unsigned; SmartScreen will warn`.
    - Both hooks stay **disabled** until M4/M5, and the PR states that the enabled path is unexercised.
  - **`publish-release`:**
    - Add `if: needs.prepare-release.outputs.publish == 'true'`.
    - Generate the manifest with `generate_updater_manifest.py --mode publish`.
    - Release body: `Kaiku_*_amd64.deb` (not `canis_*`), note "Linux: use the AppImage for auto-updates", and remove the MSI line. The image line is Task 4's.
    - Keep `draft: true`.
  - **`CHANGELOG.md` `### Changed`:** "Windows releases ship a single `-setup.exe` installer (MSI dropped)".

- [ ] **Step 5: Run the tests and confirm they pass.**
  - Run the local baseline.
  - Dry run from the PR branch: `gh workflow run release.yml --ref feature/release-pipeline-gates -f tag=v0.9.0-rc.1 -f dry_run=true`.
  - Expected: all four `build-tauri` rows are green, the log shows the notices for updater/mac/windows, there is no `publish-release` run, and no image is pushed.
  - Download the Linux artifact and confirm `Kaiku_0.9.0-rc.1_amd64.AppImage` is in it.
  - Link the run in the PR. This run proves the R-01 build and is allowed for agents: it uses no secrets and publishes nothing.

- [ ] **Step 6: Commit.** `git commit -m "feat(infra): gate releases on version, secrets and full manifest"`.

---

### Task 4: Server image to GHCR (C8)

Branch `feature/ghcr-server-image`. Depends on Task 3 (`prepare-release.outputs`).

**Files:**
- Create: `docs/admin-guide/configuration/environment.md`
- Modify: `.github/workflows/release.yml` (`build-docker`, release body), `.github/workflows/ci.yml` (`docker` job), `scripts/check_ci_guardrails.py` (+ tests), `infra/docker/AGENTS.md` (image name, publish rule; drop the stale `rust:1.75` snippets)

**Interfaces:**
- Consumes: `needs.prepare-release.outputs.{version,publish,is_prerelease}`.
- Produces:
  - **Image name** `ghcr.io/detair/kaiku-server`. Set it as `env.SERVER_IMAGE` in both workflows.
  - **Tags from `release.yml`:** `<ver>` always (`type=raw,value=${{ needs.prepare-release.outputs.version }}`). `latest` only when `is_prerelease == 'false'`.
  - **Tags from `ci.yml`:** pushes on `main` produce `main` and `sha-<short>` and **no `latest`**. `latest` means the newest final release (R-08).
  - **OCI labels:** `org.opencontainers.image.version=<ver>` and `org.opencontainers.image.source=https://github.com/Detair/kaiku`, from `docker/metadata-action`.
  - **Guardrail:** `check_env_doc(config_rs: str, doc: str, errors)`. Every `env::var("NAME")` in `server/src/config.rs` must appear as `` `NAME` `` in `environment.md`. This is a cross-stream rule: server-plan PRs that add env vars must document them.

- [ ] **Step 1: Write failing tests.**
  - `test_env_doc_lists_every_config_var`: fixture config with `env::var("FOO")`, doc without it → error naming `FOO`.
  - `test_server_image_name`: a release.yml whose metadata `images` is not `ghcr.io/detair/kaiku-server` → error.
  - `test_ci_main_never_tags_latest`: `type=raw,value=latest` in `ci.yml` → error.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `python3 -m unittest scripts.tests.test_check_ci_guardrails -v`.

- [ ] **Step 3: Write the env doc and the guardrail.**
  - Write `environment.md`: a table of name, required/default (copied from `config.rs`), and purpose, grouped server / database / Valkey / auth / storage / voice / observability.
  - State the image contract: env vars only, no config files baked in, migrations shipped in `/app/migrations`.
  - Implement `check_env_doc`.

- [ ] **Step 4: Update `build-docker` in `release.yml`.**
  - Build with `load: true` first, tagged `kaiku-server:smoke`, then run the smoke steps:
    - `docker run --rm kaiku-server:smoke 2>&1 | grep -q "DATABASE_URL must be set"`. This proves the binary runs and no config is baked in.
    - `docker run --rm --entrypoint /bin/sh kaiku-server:smoke -c 'ls -A /app'`. The output must be exactly `migrations` and `vc-server`.
    - `docker image inspect` label `org.opencontainers.image.version` equals `<ver>`.
  - Then push (`push: ${{ needs.prepare-release.outputs.publish == 'true' }}`).
  - Release body line: `` Server image: `ghcr.io/detair/kaiku-server:<ver>` ``.
  - In `ci.yml` `docker`, switch to the same image name and tags.

- [ ] **Step 5: Run the tests and confirm they pass.** Run the local baseline. The PR CI `Docker Build & Publish` must be green (build only on PRs). Run the dry-run dispatch as in Task 3: `build-docker` is green with the smoke steps passing and the log shows `push: false`.

- [ ] **Step 6: Commit.** `git commit -m "feat(infra): publish versioned server image to ghcr kaiku-server"`.

---

### Task 5: Android release build — signing inputs, no silent unsigned fallback, R8 in CI (R-03 part, A-16)

Branch `fix/android-release-signing`. Independent of Tasks 2–4. Depends on Task 1 (`version.properties`).

**Files:**
- Create: `scripts/android_verify_apk.sh` (executable)
- Modify: `mobile/android/app/build.gradle.kts:26-53`, `.github/workflows/ci.yml` (`android` job), `mobile/android/app/proguard-rules.pro` (only with rules an R8 failure proves necessary, each with a comment naming the failing class)

**Interfaces:**
- Consumes: `release_version.py android-code` (Task 1).
- Produces:
  - **Signing sources** for `signingConfigs.release`:
    1. The env vars `KAIKU_ANDROID_KEYSTORE_PATH`, `KAIKU_ANDROID_KEYSTORE_PASSWORD`, `KAIKU_ANDROID_KEY_ALIAS`, `KAIKU_ANDROID_KEY_PASSWORD`, used if all four are non-empty.
    2. Otherwise `keystore.properties` (local dev, unchanged keys).
    3. Otherwise none.
  - **Gradle property `kaiku.requireReleaseSigning=true`:** with no signing source, configuration fails with `GradleException("Release signing required but no keystore configured: set KAIKU_ANDROID_KEYSTORE_PATH/_PASSWORD, KAIKU_ANDROID_KEY_ALIAS/_PASSWORD")`. Without the property, an unsigned release build logs `logger.warn("Kaiku: release APK is UNSIGNED (no keystore configured)")`.
  - **`scripts/android_verify_apk.sh <apk> <versionName> <versionCode> [--expect-unsigned]`:**
    - Locates the newest `$ANDROID_HOME/build-tools/*/`.
    - Runs `apksigner verify --verbose --print-certs` and requires `Verified using v2 scheme (APK Signature Scheme v2): true`.
    - Runs `aapt2 dump badging` and requires `versionCode='<code>'` and `versionName='<name>'`.
    - Prints the signer SHA-256, which is not secret. With `--expect-unsigned` it instead asserts that verification fails.
    - Exits non-zero with a message naming the failed check.

- [ ] **Step 1: Write failing CI checks** in the `ci.yml` `android` job. These are the tests for this stack. Each step runs a command and asserts on its outcome.
  - `Signing is enforced when required`: `! ./gradlew help -Pkaiku.requireReleaseSigning=true --no-daemon 2> err.txt && grep -q "Release signing required" err.txt`.
  - `Release build (R8, ephemeral key)`:
    - Create the key with `keytool -genkeypair -keystore "$RUNNER_TEMP/ci.jks" -alias ci -keyalg RSA -keysize 2048 -validity 2 -dname "CN=Kaiku CI" -storepass "$P" -keypass "$P"`, where `P` is a random value from `openssl rand -hex 16` that is masked.
    - Run `./gradlew assembleRelease -Pkaiku.requireReleaseSigning=true` with the four `KAIKU_ANDROID_*` env vars.
    - Run `scripts/android_verify_apk.sh app/build/outputs/apk/release/app-release.apk "$(versionName)" "$(versionCode)"`, both read from `version.properties`.
  - `Verifier rejects the debug APK`: `! scripts/android_verify_apk.sh app/build/outputs/apk/debug/app-debug.apk 0.1.0 10099`. It must fail, because the debug key is v2-signed but the versionCode or cert check differs. If it passes, tighten the script to also require `--print-certs` not containing `CN=Android Debug`.

- [ ] **Step 2: Push and confirm the checks fail.** Expected: the first check fails because Gradle does not fail, and the release build fails because the env-var signing source does not exist.

- [ ] **Step 3: Implement the signing sources and the `kaiku.requireReleaseSigning` check** in `build.gradle.kts` (the property is read with `providers.gradleProperty`). Write `android_verify_apk.sh` (`set -eu`, usage text).

- [ ] **Step 4: Push and confirm CI passes.** The PR CI `Android Build` must be green. If R8 fails, add the minimal keep rule named in the R8 error and note it in the PR.

- [ ] **Step 5: Commit.** `git commit -m "fix(infra): enforce android release signing and verify apk in ci"`.

---

### Task 6: Android release job in `release.yml` (R-03)

Branch `feature/release-android-apk`. Depends on Tasks 3 and 5.

**Files:**
- Modify: `.github/workflows/release.yml` (new job `build-android`, `publish-release.needs` and asset globs, release body), `scripts/check_ci_guardrails.py` (+ test), `CHANGELOG.md`

**Interfaces:**
- Consumes: `prepare-release.outputs.{version,android_version_code,publish}`, `require_secrets.py`, `android_verify_apk.sh`, the signing env names from Task 5.
- Produces: the artifact `android-apk` containing `Kaiku-<ver>.apk` and `Kaiku-<ver>.apk.sha256`. The release gains the asset `Kaiku-<ver>.apk`.

- [ ] **Step 1: Write the failing guardrail test.** `test_release_android_job_contract`: the `build-android` job must have `needs: prepare-release`, the publish-scoped environment expression, `require_secrets.py` with all four `ANDROID_*` names, `-Pkaiku.requireReleaseSigning=true` gated on publish, `android_verify_apk.sh`, and the asset glob `Kaiku-*.apk` in `publish-release`.

- [ ] **Step 2: Run the test and confirm it fails.** Run `python3 -m unittest scripts.tests.test_check_ci_guardrails -v`.

- [ ] **Step 3: Implement the `build-android` job.**
  - Use the same environment expression as `build-tauri`, JDK 17 temurin and the gradle cache.
  - Dry run only: `bump-version.sh`.
  - Run `require_secrets.py --mode <mode> --output signing ANDROID_KEYSTORE_BASE64 ANDROID_KEYSTORE_PASSWORD ANDROID_KEY_ALIAS ANDROID_KEY_PASSWORD`. In publish mode this fails loudly when the secrets are missing.
  - If `signing == 'true'`: decode the base64 to `$RUNNER_TEMP/release.jks` (`umask 077`), map it to the `KAIKU_ANDROID_*` env, and add `if: always()` cleanup that removes the file.
  - Run `./gradlew assembleRelease --no-daemon`. Add `-Pkaiku.requireReleaseSigning=true` when `publish == 'true'`.
  - Verify with `android_verify_apk.sh <apk> "$VERSION" "$ANDROID_VERSION_CODE"`. In a dry run without signing, use `--expect-unsigned`.
  - Copy the output to `Kaiku-$VERSION.apk` and write the sha256 next to it.
  - Write the signer SHA-256 to `$GITHUB_STEP_SUMMARY` so the maintainer can record it for the M2 backup and `assetlinks.json`.
  - In `publish-release`, add `build-android` to `needs` and copy `Kaiku-*.apk` into `release-assets/`. Its checksum joins `checksums.txt`.
  - Release body: "Android: download `Kaiku-<ver>.apk` and allow installs from this source once. Updates install over the old version."

- [ ] **Step 4: Add the CHANGELOG entry** under `### Added`: "Releases now include a signed Android app (`Kaiku-<version>.apk`) that updates in place over earlier installs."

- [ ] **Step 5: Run the tests and confirm they pass.** Run the local baseline. Run the dry-run dispatch as in Task 3. Expected: `build-android` is green, it prints `::notice::ANDROID_KEYSTORE_BASE64, … not set — signing skipped (dry run)`, and the artifact holds `Kaiku-0.9.0-rc.1.apk` with versionCode `90001` reported by the verifier.

- [ ] **Step 6: Commit.** `git commit -m "feat(infra): build signed android apk in release workflow"`.

---

### Task 7: Release runbook and code-signing decision doc

Branch `docs/release-runbook`. Can be written after Task 3. Merge it last, so it describes what actually shipped.

**Files:**
- Create: `docs/ops/release-runbook.md`
- Modify: `docs/ops/README.md` (index row "Cutting an RC or final release"), `.github/workflows/AGENTS.md` (rewrite the stale trigger, job and secret sections to match Tasks 2–6), `.github/workflows/RELEASE_WORKFLOW_FIX.md` (replace with a 3-line pointer to the runbook, marked historical)

**Interfaces:**
- Consumes the exact names from Tasks 1–6:
  - Scripts: `bump-version.sh`, `release_version.py check`.
  - Workflow: input `dry_run`, Environment `release`.
  - Secrets: the 4 `ANDROID_*`, the 2 `TAURI_SIGNING_*` and the `APPLE_*`/`AZURE_*` sets.
  - Variables: `MACOS_SIGNING_ENABLED`, `WINDOWS_SIGNING_ENABLED`.
  - Artifacts: `ghcr.io/detair/kaiku-server:<ver>`, `Kaiku-<ver>.apk`.
- Produces: the runbook sections listed in Step 1.

- [ ] **Step 1: Write the runbook.** One section per heading, using imperative steps, with every command copy-pasteable:
  1. *Versioning rules.* Cover C7, the versionCode table for `0.9.0-rc.1`, `1.0.0-rc.1`, `1.0.0` and `1.0.1-rc.1`, and the rule "never reuse or lower a version".
  2. *One-time maintainer setup M1–M5.* Include the offline keystore command (`keytool -genkeypair -v -keystore kaiku-release.jks -alias kaiku -keyalg RSA -keysize 4096 -validity 10000`), `base64 -w0`, two backup locations (matching the off-host backup decision in the the production server plan), the Environment deployment rule, and the GHCR visibility setting.
  3. *Dry run* via `gh workflow run release.yml -f tag=… -f dry_run=true`, and what to check.
  4. *Rehearsal M6* with `v0.9.0-rc.1`, including cleanup: `gh release delete`, `git push --delete origin v0.9.0-rc.1`, and deleting the GHCR package version.
  5. *Cut an RC:* `scripts/bump-version.sh 1.0.0-rc.1`, PR, merge, `git tag -s v1.0.0-rc.1 <merge-sha>`, push the tag. Then the verification checklist: all assets present, `latest.json` has 4 platforms, the APK verifies, the image is pullable, and an older build offers the update (see M7). Then publish the draft.
  6. *Cut the final:* curate `## [1.0.0] - <date>` in CHANGELOG (picked up by `extract_section`), bump, tag `v1.0.0`.
  7. *When it fails:* re-run failed jobs, a tag on the wrong commit, a lost keystore (consequence: every relative reinstalls), updater key backup.
  8. *Code signing: options and costs.* Copy the table and recommendation from the review's lane-6 design input:
     - macOS Apple Developer ID: 99 USD/yr.
     - Windows Azure Trusted Signing: ~120 USD/yr. Certum OSS: ~70–100 €/yr. SignPath Foundation: free, weeks of lead time. OV certificate: not recommended.
     - Unsigned with a guide: 0. Include what the family sees on Windows ("More info → Run anyway") and on macOS Sequoia ("System Settings → Privacy & Security → Open Anyway").
     - How to enable each hook: set the variable and secrets, then run a dry run with `dry_run=false` on the rehearsal tag.
     - The recommendation (a Mac in the family means buy Apple; Windows ships unsigned for v1.0) is labelled **maintainer decision**.

- [ ] **Step 2: Verify.** `python3 scripts/check_docs_governance.py` passes. Every script, input, secret and variable name in the runbook is present in the repo (`grep -c` each one in `.github/` and `scripts/`, all ≥1, run as a one-off shell loop listed in the PR description).

- [ ] **Step 3: Commit.** `git commit -m "docs(infra): add release runbook and code signing options"`.

---

## Self-review notes

- **Spec coverage:**
  - Scope 4 (signed APK, versioning, sideload upgrades) is covered by Tasks 1, 5 and 6.
  - Scope 7 (`v1.0.0-rc` dry run with signed desktop, updater manifest and APK; updater key handling) is covered by Tasks 2, 3, 6, 7 and M6.
  - The restore drill and off-host backups are out of this stream (the production server plan, server plan B-06/R-04).
  - The curated changelog is covered by Task 3 `extract_section` and Task 7 §6.
- **Not here:**
  - Making `ci.yml` Android unit tests blocking (A-16 part) belongs to the Android stream, which owns those tests.
  - The macOS `Info.plist` (D-05) belongs to the desktop stream.
- **Contract notes:**
  - C8 `<version>` is interpreted as the tag without `v`.
  - C7's "Android `versionName`/`versionCode`" lives in the new `mobile/android/version.properties`, not inline in `build.gradle.kts`.
