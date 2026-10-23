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
    check_ci_never_tags_latest,
    check_env_doc,
    check_native_deps_usage,
    check_no_signing_key_in_pr_workflows,
    check_release_android,
    check_release_hygiene,
    check_release_publish_order,
    check_release_secret_scoping,
    check_release_workflow,
    check_server_image,
    check_windows_libvpx_guardrails,
)

NATIVE_DEPS_USES = "uses: ./.github/actions/setup-native-deps"

VALID_RELEASE = """
on:
  push:
    tags: ['v*.*.*']
concurrency:
  group: release-${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}
  cancel-in-progress: false
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
          load: true
          push: false
        env:
          TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - run: docker save kaiku-server:smoke | gzip > image.tar.gz
  build-android:
    needs: prepare-release
    environment: ${{ needs.prepare-release.outputs.publish == 'true' && 'release' || '' }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - id: signing
        env:
          ANDROID_KEYSTORE_BASE64: ${{ needs.prepare-release.outputs.publish == 'true' && secrets.ANDROID_KEYSTORE_BASE64 || '' }}
          ANDROID_KEYSTORE_PASSWORD: ${{ needs.prepare-release.outputs.publish == 'true' && secrets.ANDROID_KEYSTORE_PASSWORD || '' }}
          ANDROID_KEY_ALIAS: ${{ needs.prepare-release.outputs.publish == 'true' && secrets.ANDROID_KEY_ALIAS || '' }}
          ANDROID_KEY_PASSWORD: ${{ needs.prepare-release.outputs.publish == 'true' && secrets.ANDROID_KEY_PASSWORD || '' }}
        run: python3 scripts/require_secrets.py --mode dry-run --output signing ANDROID_KEYSTORE_BASE64 ANDROID_KEYSTORE_PASSWORD ANDROID_KEY_ALIAS ANDROID_KEY_PASSWORD
      - run: |
          flags=()
          if [ "$PUBLISH" = true ]; then flags+=(-Pkaiku.requireReleaseSigning=true); fi
          ./gradlew assembleRelease "${flags[@]}"
          scripts/android_verify_apk.sh app.apk 1 2
  publish-release:
    needs: [prepare-release, build-tauri, build-docker, build-android]
    if: needs.prepare-release.outputs.publish == 'true'
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: python3 scripts/generate_updater_manifest.py --mode publish
      - run: cp artifacts/android-apk/Kaiku-*.apk release-assets/
      - run: python3 scripts/verify_updater_signatures.py --mode publish
  push-image:
    needs: [prepare-release, build-tauri, build-docker, build-android, publish-release]
    if: needs.prepare-release.outputs.publish == 'true'
    runs-on: ubuntu-latest
    steps:
      - uses: docker/login-action@v3
      - run: docker push "$TAG"
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


class ReleaseAndroidJobTest(unittest.TestCase):
    def check(self, release: dict) -> list[str]:
        errors: list[str] = []
        check_release_android(release, errors)
        return errors

    def test_valid_fixture_passes(self):
        self.assertEqual(self.check(load_release()), [])

    def test_release_android_job_contract(self):
        release = load_release()
        del release["jobs"]["build-android"]
        self.assertTrue(any("build-android" in e for e in self.check(release)))

        release = load_release()
        del release["jobs"]["build-android"]["needs"]
        self.assertTrue(any("prepare-release" in e for e in self.check(release)))

        release = load_release()
        release["jobs"]["build-android"]["environment"] = "release"
        self.assertTrue(any("environment" in e for e in self.check(release)))

        release = load_release()
        step = release["jobs"]["build-android"]["steps"][1]
        step["run"] = step["run"].replace(" ANDROID_KEY_PASSWORD", "")
        self.assertTrue(any("ANDROID_KEY_PASSWORD" in e for e in self.check(release)))

        release = load_release()
        step = release["jobs"]["build-android"]["steps"][1]
        step["run"] = step["run"].replace("scripts/require_secrets.py", "true")
        self.assertTrue(any("require_secrets" in e for e in self.check(release)))

        release = load_release()
        step = release["jobs"]["build-android"]["steps"][2]
        step["run"] = step["run"].replace("-Pkaiku.requireReleaseSigning=true", "")
        self.assertTrue(any("requireReleaseSigning" in e for e in self.check(release)))

        release = load_release()
        step = release["jobs"]["build-android"]["steps"][2]
        step["run"] = step["run"].replace("android_verify_apk.sh", "true")
        self.assertTrue(any("android_verify_apk" in e for e in self.check(release)))

        release = load_release()
        pub = release["jobs"]["publish-release"]
        pub["steps"][1]["run"] = "true"
        self.assertTrue(any("Kaiku-*.apk" in e for e in self.check(release)))

        release = load_release()
        release["jobs"]["publish-release"]["needs"].remove("build-android")
        self.assertTrue(any("publish-release" in e and "build-android" in e for e in self.check(release)))

    def test_requires_signing_must_be_publish_gated(self):
        release = load_release()
        step = release["jobs"]["build-android"]["steps"][2]
        step["run"] = '''./gradlew assembleRelease -Pkaiku.requireReleaseSigning=true\nscripts/android_verify_apk.sh a 1 2\n'''
        self.assertTrue(any("publish" in e for e in self.check(release)))


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


class ReleasePublishOrderTest(unittest.TestCase):
    def check(self, release: dict) -> list[str]:
        errors: list[str] = []
        check_release_publish_order(release, errors)
        return errors

    def test_valid_fixture_passes(self):
        self.assertEqual(self.check(load_release()), [])

    def test_image_push_needs_all_builds(self):
        for need in ("build-tauri", "build-android", "build-docker", "publish-release"):
            release = load_release()
            release["jobs"]["push-image"]["needs"].remove(need)
            errors = self.check(release)
            self.assertTrue(any("push-image" in e and need in e for e in errors), (need, errors))

    def test_image_push_gated_on_publish(self):
        release = load_release()
        del release["jobs"]["push-image"]["if"]
        errors = self.check(release)
        self.assertTrue(any("push-image" in e and "gated" in e for e in errors), errors)

    def test_build_docker_must_not_push(self):
        release = load_release()
        release["jobs"]["build-docker"]["steps"][0]["with"]["push"] = "${{ needs.prepare-release.outputs.publish == 'true' }}"
        errors = self.check(release)
        self.assertTrue(any("push: false" in e for e in errors), errors)
        # The pushing build-docker job is also caught by the needs check.
        self.assertTrue(any("'build-docker' pushes" in e for e in errors), errors)

    def test_build_docker_must_not_login(self):
        release = load_release()
        release["jobs"]["build-docker"]["steps"].insert(0, {"uses": "docker/login-action@v3"})
        errors = self.check(release)
        self.assertTrue(any("log in" in e for e in errors), errors)

    def test_build_docker_must_export_image(self):
        release = load_release()
        release["jobs"]["build-docker"]["steps"].pop()
        errors = self.check(release)
        self.assertTrue(any("docker save" in e for e in errors), errors)

    def test_some_job_must_push(self):
        release = load_release()
        del release["jobs"]["push-image"]
        errors = self.check(release)
        self.assertTrue(any("no job pushes" in e for e in errors), errors)

    def test_concurrency_keyed_by_tag_and_never_cancels(self):
        release = load_release()
        del release["concurrency"]
        self.assertTrue(any("concurrency" in e for e in self.check(release)))
        release = load_release()
        release["concurrency"]["group"] = "release"
        self.assertTrue(any("concurrency.group" in e for e in self.check(release)))
        release = load_release()
        release["concurrency"]["cancel-in-progress"] = True
        self.assertTrue(any("cancel-in-progress" in e for e in self.check(release)))

    def test_publish_verifies_updater_signatures_after_manifest(self):
        release = load_release()
        steps = release["jobs"]["publish-release"]["steps"]
        steps.pop(2)
        self.assertTrue(any("verify_updater_signatures.py" in e for e in self.check(release)))
        release = load_release()
        steps = release["jobs"]["publish-release"]["steps"]
        steps[0], steps[2] = steps[2], steps[0]
        self.assertTrue(any("after generating" in e for e in self.check(release)))


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


CONFIG_FIXTURE = """
database_url: env::var("FOO").context("FOO must be set")?,
s3_bucket: env_opt("BAR_BAZ"),
other: std::env::var("FOO").ok(),
"""

IMAGE_WORKFLOW = """
env:
  SERVER_IMAGE: ghcr.io/detair/kaiku-server
jobs:
  build-docker:
    steps:
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: %s
          tags: |
            type=raw,value=${{ needs.prepare-release.outputs.version }}
            type=raw,value=latest,enable=${{ needs.prepare-release.outputs.is_prerelease == 'false' }}
"""


class EnvDocTest(unittest.TestCase):
    def test_env_doc_lists_every_config_var(self):
        errors: list[str] = []
        check_env_doc(CONFIG_FIXTURE, "| `BAR_BAZ` | unset | bucket |\n", errors)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("FOO", errors[0])

    def test_documented_vars_pass(self):
        errors: list[str] = []
        check_env_doc(CONFIG_FIXTURE, "`FOO` and `BAR_BAZ`", errors)
        self.assertEqual(errors, [])

    def test_bare_mention_is_not_documentation(self):
        errors: list[str] = []
        check_env_doc(CONFIG_FIXTURE, "FOO BAR_BAZ", errors)
        self.assertEqual(len(errors), 2, errors)


class ServerImageTest(unittest.TestCase):
    def check(self, images: str) -> list[str]:
        errors: list[str] = []
        check_server_image(yaml.safe_load(IMAGE_WORKFLOW % images), "release.yml", errors)
        return errors

    def test_server_image_name(self):
        errors = self.check("ghcr.io/${{ github.repository }}/server")
        self.assertTrue(any("ghcr.io/detair/kaiku-server" in e for e in errors), errors)

    def test_env_reference_resolves(self):
        self.assertEqual(self.check("${{ env.SERVER_IMAGE }}"), [])

    def test_literal_name_passes(self):
        self.assertEqual(self.check("ghcr.io/detair/kaiku-server"), [])

    def test_no_metadata_step_is_an_error(self):
        errors: list[str] = []
        check_server_image({"jobs": {"a": {"steps": [{"run": "true"}]}}}, "release.yml", errors)
        self.assertTrue(errors)

    def test_latest_must_depend_on_prerelease(self):
        wf = yaml.safe_load(IMAGE_WORKFLOW % "${{ env.SERVER_IMAGE }}")
        step = wf["jobs"]["build-docker"]["steps"][0]
        step["with"]["tags"] = "type=raw,value=latest\n"
        errors: list[str] = []
        check_server_image(wf, "release.yml", errors)
        self.assertTrue(any("latest" in e and "is_prerelease" in e for e in errors), errors)

    def test_ci_main_never_tags_latest(self):
        errors: list[str] = []
        check_ci_never_tags_latest("tags: |\n  type=raw,value=latest,enable={{is_default_branch}}\n", errors)
        self.assertTrue(any("latest" in e for e in errors), errors)

    def test_ci_without_latest_passes(self):
        errors: list[str] = []
        check_ci_never_tags_latest("tags: |\n  type=ref,event=branch\n  type=sha\n", errors)
        self.assertEqual(errors, [])


class RepoTest(unittest.TestCase):
    def test_repo_passes(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = guardrails.main()
        self.assertEqual(rc, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
