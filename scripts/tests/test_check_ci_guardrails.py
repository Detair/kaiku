"""Tests for scripts/check_ci_guardrails.py (release/native-deps guardrails)."""
from __future__ import annotations

import contextlib
import io
import sys
import unittest
from pathlib import Path

import yaml

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))

import check_ci_guardrails as guardrails  # noqa: E402
from check_ci_guardrails import (  # noqa: E402
    check_native_deps_usage,
    check_no_signing_key_in_pr_workflows,
    check_release_hygiene,
    check_release_secret_scoping,
    check_release_workflow,
    check_windows_libvpx_guardrails,
)

NATIVE_DEPS_USES = "uses: ./.github/actions/setup-native-deps"

VALID_RELEASE = """
on:
  push:
    tags: ['v*.*.*']
permissions:
  contents: read
jobs:
  prepare-release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - id: version
        run: |
          python3 scripts/release_version.py parse "$TAG" > "$RUNNER_TEMP/version.env"
          cat "$RUNNER_TEMP/version.env" >> "$GITHUB_OUTPUT"
  build-tauri:
    needs: prepare-release
    defaults:
      run:
        shell: bash
    strategy:
      matrix:
        include:
          - {os: ubuntu-24.04, target: x86_64-unknown-linux-gnu, bundles: 'appimage,deb'}
          - {os: windows-latest, target: x86_64-pc-windows-msvc, bundles: nsis}
          - {os: macos-15-intel, target: x86_64-apple-darwin, bundles: 'app,dmg'}
          - {os: macos-latest, target: aarch64-apple-darwin, bundles: 'app,dmg'}
    runs-on: ${{ matrix.os }}
    environment: ${{ needs.prepare-release.outputs.publish == 'true' && 'release' || '' }}
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-native-deps
      - run: |
          for var in APPLE_CERTIFICATE APPLE_SIGNING_IDENTITY; do
            [ -n "${!var}" ] || unset "$var"
          done
          bun run tauri build
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ needs.prepare-release.outputs.publish == 'true' && secrets.TAURI_SIGNING_PRIVATE_KEY || '' }}
  build-docker:
    needs: prepare-release
    runs-on: ubuntu-latest
    steps:
      - uses: docker/build-push-action@v5
        with:
          context: .
          file: infra/docker/Dockerfile
        env:
          TOKEN: ${{ secrets.GITHUB_TOKEN }}
  publish-release:
    needs: [prepare-release, build-tauri, build-docker]
    if: needs.prepare-release.outputs.publish == 'true'
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: python3 scripts/generate_updater_manifest.py --mode publish
"""

TAURI_JOB_TEMPLATE = """
jobs:
  {job}:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
{extra}
      - run: bun run tauri build
"""


def load_release() -> dict:
    return yaml.safe_load(VALID_RELEASE)


def workflow_with_job(job: str, with_action: bool) -> str:
    extra = "      - uses: ./.github/actions/setup-native-deps" if with_action else ""
    return TAURI_JOB_TEMPLATE.format(job=job, extra=extra)


class ReleaseWorkflowTest(unittest.TestCase):
    def check(self, release: dict) -> list[str]:
        errors: list[str] = []
        check_release_workflow(release, REPO_ROOT, errors)
        return errors

    def test_valid_fixture_passes(self):
        self.assertEqual(self.check(load_release()), [])

    def test_release_dockerfile_must_exist(self):
        release = load_release()
        release["jobs"]["build-docker"]["steps"][0]["with"]["file"] = "infra/docker/server.Dockerfile"
        errors = self.check(release)
        self.assertTrue(any("Dockerfile" in e for e in errors), errors)

    def test_release_rejects_intel_target_on_arm_runner(self):
        release = load_release()
        rows = release["jobs"]["build-tauri"]["strategy"]["matrix"]["include"]
        rows[2]["os"] = "macos-latest"
        errors = self.check(release)
        self.assertTrue(any("macos-15-intel" in e for e in errors), errors)

    def test_release_one_row_per_target(self):
        release = load_release()
        rows = release["jobs"]["build-tauri"]["strategy"]["matrix"]["include"]
        rows.append({"os": "ubuntu-24.04", "target": "x86_64-unknown-linux-gnu", "bundles": "deb"})
        errors = self.check(release)
        self.assertTrue(any("duplicate target" in e for e in errors), errors)

    def test_release_rows_need_bundles(self):
        release = load_release()
        del release["jobs"]["build-tauri"]["strategy"]["matrix"]["include"][1]["bundles"]
        errors = self.check(release)
        self.assertTrue(any("bundles" in e for e in errors), errors)

    def test_release_build_shell_is_bash(self):
        release = load_release()
        del release["jobs"]["build-tauri"]["defaults"]
        errors = self.check(release)
        self.assertTrue(any("shell" in e for e in errors), errors)

    def test_release_builds_need_prepare(self):
        for job in ("build-tauri", "build-docker"):
            with self.subTest(job=job):
                release = load_release()
                del release["jobs"][job]["needs"]
                errors = self.check(release)
                self.assertTrue(any(job in e and "prepare-release" in e for e in errors), errors)


class ReleaseSecretScopingTest(unittest.TestCase):
    def check(self, release: dict) -> list[str]:
        errors: list[str] = []
        check_release_secret_scoping(release, errors)
        return errors

    def test_valid_fixture_passes(self):
        self.assertEqual(self.check(load_release()), [])

    def test_release_signing_env_only_in_publish(self):
        release = load_release()
        del release["jobs"]["build-tauri"]["environment"]
        errors = self.check(release)
        self.assertTrue(any("build-tauri" in e and "environment" in e for e in errors), errors)

        release = load_release()
        release["jobs"]["build-tauri"]["environment"] = "release"
        errors = self.check(release)
        self.assertTrue(any("build-tauri" in e and "publish" in e for e in errors), errors)

        release = load_release()
        step = release["jobs"]["build-tauri"]["steps"][2]
        step["env"]["TAURI_SIGNING_PRIVATE_KEY"] = "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}"
        errors = self.check(release)
        self.assertTrue(any("TAURI_SIGNING_PRIVATE_KEY" in e for e in errors), errors)

    def test_script_names_are_not_secret_refs(self):
        release = load_release()
        release["jobs"]["build-docker"]["steps"].append({"run": "python3 scripts/require_secrets.py --mode dry-run"})
        self.assertEqual(self.check(release), [])

    def test_github_token_needs_no_environment(self):
        release = load_release()
        self.assertNotIn("environment", release["jobs"]["build-docker"])
        self.assertEqual(self.check(release), [])


class ReleaseHygieneTest(unittest.TestCase):
    def check(self, text: str) -> list[str]:
        errors: list[str] = []
        check_release_hygiene(text, yaml.safe_load(text), errors)
        return errors

    def test_valid_fixture_passes(self):
        self.assertEqual(self.check(VALID_RELEASE), [])

    def test_release_declares_permissions(self):
        text = VALID_RELEASE.replace("permissions:\n  contents: read\njobs:", "jobs:")
        errors = self.check(text)
        self.assertTrue(any("permissions" in e for e in errors), errors)

    def test_release_top_level_permissions_read_only(self):
        text = VALID_RELEASE.replace("permissions:\n  contents: read\njobs:", "permissions:\n  contents: write\njobs:")
        errors = self.check(text)
        self.assertTrue(any("permissions" in e for e in errors), errors)

    def test_release_has_no_docs_governance_step(self):
        text = VALID_RELEASE + "      - run: python3 scripts/check_docs_governance.py\n"
        errors = self.check(text)
        self.assertTrue(any("check_docs_governance.py" in e for e in errors), errors)

    def test_prepare_parse_cannot_mask_exit_code(self):
        piped = 'python3 scripts/release_version.py parse "$TAG" | tee -a "$GITHUB_OUTPUT"\n'
        release = yaml.safe_load(VALID_RELEASE)
        release["jobs"]["prepare-release"]["steps"][1]["run"] = piped
        errors: list[str] = []
        check_release_hygiene(VALID_RELEASE, release, errors)
        self.assertTrue(any("release_version.py parse" in e for e in errors), errors)

        release["jobs"]["prepare-release"]["steps"].pop(1)
        errors = []
        check_release_hygiene(VALID_RELEASE, release, errors)
        self.assertTrue(any("release_version.py parse" in e for e in errors), errors)

    def test_build_unsets_empty_signing_env(self):
        release = yaml.safe_load(VALID_RELEASE)
        release["jobs"]["build-tauri"]["steps"][2]["run"] = "bun run tauri build\n"
        errors: list[str] = []
        check_release_hygiene(VALID_RELEASE, release, errors)
        self.assertTrue(any("unset" in e for e in errors), errors)

    def test_publish_release_gated_on_publish(self):
        text = VALID_RELEASE.replace("    if: needs.prepare-release.outputs.publish == 'true'\n", "")
        errors = self.check(text)
        self.assertTrue(any("publish-release" in e for e in errors), errors)


class NativeDepsUsageTest(unittest.TestCase):
    def test_native_deps_action_used_everywhere(self):
        workflows = {
            "ci.yml": workflow_with_job("tauri", with_action=False),
            "tauri-build.yml": workflow_with_job("build", with_action=True),
            "release.yml": workflow_with_job("build-tauri", with_action=True),
        }
        errors: list[str] = []
        check_native_deps_usage(workflows, errors)
        self.assertTrue(any("ci.yml" in e for e in errors), errors)
        self.assertFalse(any("tauri-build.yml" in e or "release.yml" in e for e in errors), errors)

    def test_native_deps_all_present_passes(self):
        workflows = {
            "ci.yml": workflow_with_job("tauri", with_action=True),
            "tauri-build.yml": workflow_with_job("build", with_action=True),
            "release.yml": workflow_with_job("build-tauri", with_action=True),
        }
        errors: list[str] = []
        check_native_deps_usage(workflows, errors)
        self.assertEqual(errors, [])

    def test_inline_libvpx_install_rejected(self):
        text = workflow_with_job("tauri", with_action=True) + "      - run: vcpkg install libvpx:x64-windows-static-md\n"
        errors: list[str] = []
        check_native_deps_usage({"ci.yml": text}, errors)
        self.assertTrue(any("ci.yml" in e for e in errors), errors)


class LibvpxActionTest(unittest.TestCase):
    def test_libvpx_tokens_checked_in_action(self):
        action_text = (REPO_ROOT / ".github/actions/setup-native-deps/action.yml").read_text(encoding="utf-8")
        self.assertIn("VPX_STATIC=1", action_text)
        errors: list[str] = []
        check_windows_libvpx_guardrails(action_text.replace("VPX_STATIC=1", ""), errors)
        self.assertTrue(any("action.yml" in e and "VPX_STATIC=1" in e for e in errors), errors)


class SigningKeyTest(unittest.TestCase):
    def test_pr_workflows_never_reference_signing_key(self):
        for name in ("ci.yml", "tauri-build.yml"):
            with self.subTest(workflow=name):
                errors: list[str] = []
                text = "env:\n  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n"
                check_no_signing_key_in_pr_workflows({name: text}, errors)
                self.assertTrue(any(name in e for e in errors), errors)

    def test_clean_pr_workflow_passes(self):
        errors: list[str] = []
        check_no_signing_key_in_pr_workflows({"ci.yml": "jobs: {}\n"}, errors)
        self.assertEqual(errors, [])


class RepoTest(unittest.TestCase):
    def test_repo_passes(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = guardrails.main()
        self.assertEqual(rc, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
