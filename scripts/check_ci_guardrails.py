#!/usr/bin/env python3
from __future__ import annotations

import json
import plistlib
import re
import sys
from pathlib import Path
from typing import Any

import yaml


ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"
TAURI_WORKFLOW = ROOT / ".github/workflows/tauri-build.yml"
RELEASE_WORKFLOW = ROOT / ".github/workflows/release.yml"
WORKFLOW_DIR = ROOT / ".github/workflows"
NATIVE_DEPS_ACTION = ROOT / ".github/actions/setup-native-deps/action.yml"
NATIVE_DEPS_ACTION_REF = "./.github/actions/setup-native-deps"
SETUP_INTEGRATION = ROOT / "server/tests/integration/setup_integration.rs"
UPLOADS_HTTP = ROOT / "server/tests/integration/uploads_http.rs"
TAURI_DIR = ROOT / "client/src-tauri"
TAURI_CONF = TAURI_DIR / "tauri.conf.json"
INFO_PLIST = TAURI_DIR / "Info.plist"
MACOS_ENTITLEMENT_KEYS = [
    "com.apple.security.device.audio-input",
    "com.apple.security.device.camera",
]
MACOS_USAGE_KEYS = ["NSMicrophoneUsageDescription", "NSCameraUsageDescription"]
CONFIG_RS = ROOT / "server/src/config.rs"
ENV_DOC = ROOT / "docs/admin-guide/configuration/environment.md"
SERVER_IMAGE = "ghcr.io/detair/kaiku-server"

WINDOWS_LIBVPX_TOKENS = [
    "vcpkg install libvpx:x64-windows-static-md",
    "choco install -y pkgconfiglite",
    "VPX_LIB_DIR=",
    "VPX_INCLUDE_DIR=",
    "VPX_VERSION=1.15.2",
    "VPX_STATIC=1",
]


def read_text(path: Path, errors: list[str]) -> str:
    if not path.exists():
        errors.append(f"Missing required file: {path.relative_to(ROOT)}")
        return ""
    return path.read_text(encoding="utf-8")


def require_token(content: str, token: str, label: str, errors: list[str]) -> None:
    if token not in content:
        errors.append(f"{label}: missing required token: {token}")


def forbid_pattern(content: str, pattern: str, label: str, errors: list[str]) -> None:
    if re.search(pattern, content, flags=re.MULTILINE | re.DOTALL):
        errors.append(f"{label}: forbidden pattern matched: {pattern}")


# Workflow file name -> job that builds the Tauri client and must use the shared action.
NATIVE_DEPS_JOBS = {
    "ci.yml": "tauri",
    "tauri-build.yml": "build",
    "release.yml": "build-tauri",
}

# Tokens that only belong in the composite action; inlining them again means drift.
INLINE_NATIVE_DEPS_TOKENS = [
    "vcpkg install libvpx",
    "choco install -y pkgconfiglite",
    "brew install pkg-config libvpx",
]

# Runner image -> the only Apple target it may build (Intel vs. Apple Silicon).
MACOS_RUNNER_TARGETS = {
    "macos-15-intel": "x86_64-apple-darwin",
    "macos-13": "x86_64-apple-darwin",
    "macos-latest": "aarch64-apple-darwin",
    "macos-14": "aarch64-apple-darwin",
    "macos-15": "aarch64-apple-darwin",
}

SIGNING_KEY_TOKEN = "TAURI_SIGNING_PRIVATE_KEY"

# Every non-GITHUB_TOKEN secret in release.yml lives in Environment 'release'
# and may only be read on a publish run.
EXPRESSION_RE = re.compile(r"\$\{\{(.*?)\}\}", re.DOTALL)
SECRET_REF_RE = re.compile(r"\bsecrets\.([A-Za-z_][A-Za-z0-9_]*)")
PUBLISH_GATE = "needs.prepare-release.outputs.publish == 'true'"


USES_RE = re.compile(r"^\s*-?\s*uses:\s*([^\s#]+)", re.MULTILINE)
PINNED_RE = re.compile(r"@[0-9a-f]{40}$")


def _truthy(value: Any) -> bool:
    """YAML `true` or an expression such as `${{ matrix.allow_failure }}` counts as set."""
    if isinstance(value, str):
        return value.strip().lower() not in ("", "false")
    return bool(value)


def check_workflow_permissions(workflows: dict[str, dict], errors: list[str]) -> None:
    """Every workflow declares top-level permissions; nothing grants write-all."""
    for name, wf in workflows.items():
        if "permissions" not in wf:
            errors.append(f"{name}: missing top-level permissions (use `permissions: contents: read`)")
        elif wf["permissions"] == "write-all":
            errors.append(f"{name}: top-level permissions must not be write-all")
        for job_name, job in (wf.get("jobs") or {}).items():
            if isinstance(job, dict) and job.get("permissions") == "write-all":
                errors.append(f"{name}: job '{job_name}' grants write-all")


def check_actions_pinned(workflow_texts: dict[str, str], errors: list[str]) -> None:
    """Every non-local `uses:` is pinned to a full 40-character commit SHA."""
    for name, text in workflow_texts.items():
        for ref in USES_RE.findall(text):
            if ref.startswith("./"):
                continue
            if not PINNED_RE.search(ref):
                errors.append(f"{name}: action not pinned to a commit SHA: {ref}")


def check_no_advisory_gates(ci: dict, errors: list[str]) -> None:
    """Gitleaks, Android unit tests and the Windows Tauri build must block."""
    for job_name, job in (ci.get("jobs") or {}).items():
        if not isinstance(job, dict):
            continue
        job_advisory = _truthy(job.get("continue-on-error"))
        for step in _job_steps(job):
            advisory = _truthy(step.get("continue-on-error"))
            uses = str(step.get("uses") or "")
            run = str(step.get("run") or "")
            if advisory and "gitleaks" in uses:
                errors.append(f"{job_name}: gitleaks step must not set continue-on-error")
            if advisory and "testDebugUnitTest" in run:
                errors.append(f"{job_name}: Android unit tests must not set continue-on-error")
        if not job_advisory:
            continue
        for entry in ((job.get("strategy") or {}).get("matrix") or {}).get("include") or []:
            target = str(entry.get("platform") or entry.get("os") or "")
            if target.startswith("windows") and _truthy(entry.get("allow_failure")):
                errors.append(f"{job_name}: Windows Tauri build must not be allowed to fail ({target})")
        if str(job.get("runs-on") or "").startswith("windows") and job.get("continue-on-error") is True:
            errors.append(f"{job_name}: Windows job must not set continue-on-error")


def check_windows_libvpx_guardrails(action_text: str, errors: list[str]) -> None:
    for token in WINDOWS_LIBVPX_TOKENS:
        require_token(action_text, token, "action.yml", errors)

    forbid_pattern(action_text, r"PKG_CONFIG=.*pkg-config|PKG_CONFIG=.*pkgconf", "action.yml", errors)


def _job_steps(job: Any) -> list[dict]:
    if not isinstance(job, dict):
        return []
    steps = job.get("steps") or []
    return [step for step in steps if isinstance(step, dict)]


def _needs(job: dict) -> list[str]:
    needs = job.get("needs") or []
    return [needs] if isinstance(needs, str) else list(needs)


def check_native_deps_usage(workflows: dict[str, str], errors: list[str]) -> None:
    for name, text in workflows.items():
        for token in INLINE_NATIVE_DEPS_TOKENS:
            if token in text:
                errors.append(
                    f"{name}: inline native dependency install found ({token}); "
                    f"use {NATIVE_DEPS_ACTION_REF} instead"
                )

        job_name = NATIVE_DEPS_JOBS.get(name)
        if job_name is None:
            continue
        try:
            parsed = yaml.safe_load(text) or {}
        except yaml.YAMLError as exc:
            errors.append(f"{name}: invalid YAML: {exc}")
            continue
        job = (parsed.get("jobs") or {}).get(job_name)
        if job is None:
            errors.append(f"{name}: missing job '{job_name}'")
            continue

        uses = [step.get("uses", "") for step in _job_steps(job)]
        if NATIVE_DEPS_ACTION_REF not in uses:
            errors.append(f"{name}: job '{job_name}' must use: {NATIVE_DEPS_ACTION_REF}")
            continue
        checkout = next((i for i, u in enumerate(uses) if u.startswith("actions/checkout@")), None)
        if checkout is None or checkout > uses.index(NATIVE_DEPS_ACTION_REF):
            errors.append(f"{name}: job '{job_name}' must run actions/checkout before {NATIVE_DEPS_ACTION_REF}")


def check_release_workflow(release: dict, repo_root: Path, errors: list[str]) -> None:
    label = "release.yml"
    jobs = release.get("jobs") or {}

    for job_name in ("build-tauri", "build-docker"):
        job = jobs.get(job_name)
        if not isinstance(job, dict):
            errors.append(f"{label}: missing job '{job_name}'")
            continue
        if "prepare-release" not in _needs(job):
            errors.append(f"{label}: job '{job_name}' must have needs: prepare-release")

    tauri = jobs.get("build-tauri")
    if isinstance(tauri, dict):
        shell = ((tauri.get("defaults") or {}).get("run") or {}).get("shell")
        if shell != "bash":
            errors.append(f"{label}: build-tauri must set defaults.run.shell: bash (got {shell!r})")

        rows = ((tauri.get("strategy") or {}).get("matrix") or {}).get("include") or []
        if not rows:
            errors.append(f"{label}: build-tauri matrix.include is empty")
        seen: set[str] = set()
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                errors.append(f"{label}: build-tauri matrix row {index} is not a mapping")
                continue
            for key in ("os", "target", "bundles"):
                if not row.get(key):
                    errors.append(f"{label}: build-tauri matrix row {index} missing '{key}'")
            target = row.get("target")
            os_name = row.get("os")
            if target in seen:
                errors.append(f"{label}: build-tauri matrix has duplicate target {target}")
            if target:
                seen.add(target)
            if isinstance(target, str) and target.endswith("-apple-darwin"):
                expected = MACOS_RUNNER_TARGETS.get(os_name)
                if expected != target:
                    runner = "macos-15-intel" if target.startswith("x86_64") else "macos-latest"
                    errors.append(
                        f"{label}: target {target} must build on {runner} (got os {os_name!r})"
                    )

    docker = jobs.get("build-docker")
    for step in _job_steps(docker):
        if str(step.get("uses", "")).startswith("docker/build-push-action@"):
            dockerfile = (step.get("with") or {}).get("file")
            if not dockerfile:
                errors.append(f"{label}: build-docker build-push step must set an explicit Dockerfile 'file'")
            elif not (repo_root / dockerfile).is_file():
                errors.append(f"{label}: build-docker Dockerfile not found: {dockerfile}")


def _strings(node: Any):
    if isinstance(node, str):
        yield node
    elif isinstance(node, dict):
        for value in node.values():
            yield from _strings(value)
    elif isinstance(node, list):
        for value in node:
            yield from _strings(value)


def _secret_names(text: str) -> set[str]:
    """Secrets referenced inside ${{ }} expressions, minus GITHUB_TOKEN."""
    names = {name for expr in EXPRESSION_RE.findall(text) for name in SECRET_REF_RE.findall(expr)}
    return names - {"GITHUB_TOKEN"}


def check_release_secret_scoping(release: dict, errors: list[str]) -> None:
    label = "release.yml"
    for job_name, job in (release.get("jobs") or {}).items():
        if not isinstance(job, dict):
            continue
        refs = [text for text in _strings(job) if _secret_names(text)]
        if not refs:
            continue
        env = job.get("environment")
        if isinstance(env, dict):
            env = env.get("name")
        if not isinstance(env, str):
            errors.append(f"{label}: job '{job_name}' reads release secrets but sets no environment")
        elif "release" not in env or PUBLISH_GATE not in env:
            errors.append(
                f"{label}: job '{job_name}' environment must be 'release' only on publish "
                f"(expression with {PUBLISH_GATE}), got {env!r}"
            )
        for text in refs:
            if PUBLISH_GATE not in text:
                names = sorted(_secret_names(text))
                errors.append(
                    f"{label}: job '{job_name}' reads {', '.join(names)} without the publish gate ({PUBLISH_GATE})"
                )


ANDROID_SECRETS = [
    "ANDROID_KEYSTORE_BASE64",
    "ANDROID_KEYSTORE_PASSWORD",
    "ANDROID_KEY_ALIAS",
    "ANDROID_KEY_PASSWORD",
]
ANDROID_SIGNING_FLAG = "-Pkaiku.requireReleaseSigning=true"


def check_release_android(release: dict, errors: list[str]) -> None:
    label = "release.yml"
    jobs = release.get("jobs") or {}
    job = jobs.get("build-android")
    if not isinstance(job, dict):
        errors.append(f"{label}: missing job 'build-android'")
        return
    if "prepare-release" not in _needs(job):
        errors.append(f"{label}: job 'build-android' must have needs: prepare-release")
    env = job.get("environment")
    if not isinstance(env, str) or "release" not in env or PUBLISH_GATE not in env:
        errors.append(
            f"{label}: job 'build-android' environment must be 'release' only on publish "
            f"(expression with {PUBLISH_GATE}), got {env!r}"
        )

    runs = [str(step.get("run", "")) for step in _job_steps(job)]
    gate = [run for run in runs if "require_secrets.py" in run]
    if not gate:
        errors.append(f"{label}: build-android must gate signing with require_secrets.py")
    else:
        for name in ANDROID_SECRETS:
            if not any(name in run for run in gate):
                errors.append(f"{label}: build-android require_secrets.py must name {name}")

    gradle = [run for run in runs if "assembleRelease" in run]
    if not gradle:
        errors.append(f"{label}: build-android must run gradlew assembleRelease")
    elif not any(ANDROID_SIGNING_FLAG in run for run in gradle):
        errors.append(f"{label}: build-android must pass {ANDROID_SIGNING_FLAG} on publish runs")
    elif any(
        ANDROID_SIGNING_FLAG in line and "publish" not in line.lower()
        and not any("publish" in prev.lower() for prev in run.splitlines()[max(0, i - 3):i])
        for run in gradle for i, line in enumerate(run.splitlines())
    ):
        errors.append(f"{label}: build-android {ANDROID_SIGNING_FLAG} must be gated on publish")

    if not any("android_verify_apk.sh" in run for run in runs):
        errors.append(f"{label}: build-android must verify the APK with android_verify_apk.sh")

    publish = jobs.get("publish-release")
    if isinstance(publish, dict):
        if "build-android" not in _needs(publish):
            errors.append(f"{label}: publish-release must need build-android")
        if not any("Kaiku-*.apk" in run for run in (str(s.get("run", "")) for s in _job_steps(publish))):
            errors.append(f"{label}: publish-release must collect Kaiku-*.apk into release assets")


def check_release_hygiene(release_text: str, release: dict, errors: list[str]) -> None:
    label = "release.yml"
    permissions = release.get("permissions")
    if not isinstance(permissions, dict):
        errors.append(f"{label}: top-level permissions must be declared (contents: read)")
    elif any(value == "write" for value in permissions.values()):
        errors.append(f"{label}: top-level permissions must be read-only; grant write per job")

    if "check_docs_governance.py" in release_text:
        errors.append(f"{label}: must not run check_docs_governance.py (CI gate, not a release gate)")

    jobs = release.get("jobs") or {}
    parse_steps = [
        str(step.get("run", "")) for step in _job_steps(jobs.get("prepare-release"))
        if "release_version.py parse" in str(step.get("run", ""))
    ]
    if not parse_steps:
        errors.append(f"{label}: prepare-release must run release_version.py parse on the tag")
    elif any("|" in run for run in parse_steps):
        errors.append(
            f"{label}: prepare-release must not pipe release_version.py parse "
            "(no pipefail: a pipe masks the invalid-tag exit code)"
        )

    builds = [
        str(step.get("run", "")) for step in _job_steps(jobs.get("build-tauri"))
        if "tauri build" in str(step.get("run", ""))
    ]
    if not builds or any('unset "$var"' not in run for run in builds):
        errors.append(
            f"{label}: build-tauri must unset empty signing env vars before tauri build "
            "(tauri-bundler checks presence, not content)"
        )

    publish = jobs.get("publish-release")
    if not isinstance(publish, dict):
        errors.append(f"{label}: missing job 'publish-release'")
    elif "needs.prepare-release.outputs.publish == 'true'" not in str(publish.get("if", "")):
        errors.append(f"{label}: publish-release must run only if {PUBLISH_GATE}")


IMAGE_PUSH_NEEDS = ("prepare-release", "build-tauri", "build-docker", "build-android", "publish-release")


def check_release_publish_order(release: dict, errors: list[str]) -> None:
    """Nothing public (image, `latest`) before every build and the release assembly succeeded."""
    label = "release.yml"
    jobs = release.get("jobs") or {}

    concurrency = release.get("concurrency")
    if not isinstance(concurrency, dict) or "inputs.tag" not in str(concurrency.get("group", "")) \
            or "github.ref_name" not in str(concurrency.get("group", "")):
        errors.append(f"{label}: top-level concurrency.group must be keyed by the tag (inputs.tag / github.ref_name)")
    elif concurrency.get("cancel-in-progress") is not False:
        errors.append(f"{label}: concurrency.cancel-in-progress must be false (never cancel a publish run)")

    docker = jobs.get("build-docker")
    docker_steps = _job_steps(docker)
    for step in docker_steps:
        uses = str(step.get("uses", ""))
        if uses.startswith("docker/login-action@"):
            errors.append(f"{label}: build-docker must not log in to a registry (push happens in a later job)")
        if uses.startswith("docker/build-push-action@") and (step.get("with") or {}).get("push") not in (None, False):
            errors.append(f"{label}: build-docker must build with push: false (push only after all builds)")
    if isinstance(docker, dict) and not any("docker save" in str(s.get("run", "")) for s in docker_steps):
        errors.append(f"{label}: build-docker must export the smoke-tested image with docker save")

    pushers = [
        name for name, job in jobs.items()
        if any(
            "docker push" in str(s.get("run", ""))
            or (str(s.get("uses", "")).startswith("docker/build-push-action@")
                and (s.get("with") or {}).get("push") not in (None, False))
            for s in _job_steps(job)
        )
    ]
    if not pushers:
        errors.append(f"{label}: no job pushes the server image")
    for name in pushers:
        job = jobs[name]
        missing = [need for need in IMAGE_PUSH_NEEDS if need not in _needs(job)]
        if missing:
            errors.append(f"{label}: job '{name}' pushes the image but does not need: {', '.join(missing)}")
        if PUBLISH_GATE not in str(job.get("if", "")):
            errors.append(f"{label}: job '{name}' pushes the image but is not gated on {PUBLISH_GATE}")

    publish_runs = [str(s.get("run", "")) for s in _job_steps(jobs.get("publish-release"))]
    manifest = next((i for i, run in enumerate(publish_runs) if "generate_updater_manifest.py" in run), None)
    verify = next((i for i, run in enumerate(publish_runs) if "verify_updater_signatures.py" in run), None)
    if verify is None:
        errors.append(f"{label}: publish-release must verify updater signatures with verify_updater_signatures.py")
    elif manifest is not None and verify < manifest:
        errors.append(f"{label}: publish-release must verify signatures after generating latest.json")


ENV_READ_RE = re.compile(r'\b(?:env::var|env_opt)\(\s*"([A-Z][A-Z0-9_]*)"')


def check_env_doc(config_rs: str, doc: str, errors: list[str]) -> None:
    """Every env var read in config.rs must appear as `NAME` in environment.md."""
    for name in sorted(set(ENV_READ_RE.findall(config_rs))):
        if f"`{name}`" not in doc:
            errors.append(
                f"environment.md: config.rs reads {name} but the doc does not list `{name}` "
                "(document new env vars in docs/admin-guide/configuration/environment.md)"
            )


def check_server_image(workflow: dict, label: str, errors: list[str]) -> None:
    """docker/metadata-action must target the GHCR server image; release latest needs is_prerelease."""
    env = workflow.get("env") or {}
    found = False
    for job in (workflow.get("jobs") or {}).values():
        for step in _job_steps(job):
            if not str(step.get("uses", "")).startswith("docker/metadata-action"):
                continue
            found = True
            with_ = step.get("with") or {}
            images = str(with_.get("images", ""))
            images = re.sub(
                r"\$\{\{\s*env\.(\w+)\s*\}\}", lambda m: str(env.get(m.group(1), m.group(0))), images
            )
            if images.strip() != SERVER_IMAGE:
                errors.append(f"{label}: metadata images must be {SERVER_IMAGE}, got {images.strip()!r}")
            for line in str(with_.get("tags", "")).splitlines():
                if "value=latest" in line and "is_prerelease" not in line:
                    errors.append(f"{label}: latest tag must be enabled only when is_prerelease == 'false': {line.strip()}")
    if not found:
        errors.append(f"{label}: no docker/metadata-action step found for the server image")


def check_ci_never_tags_latest(ci_text: str, errors: list[str]) -> None:
    if re.search(r"value=latest", ci_text):
        errors.append("ci.yml: main builds must never tag latest (latest = newest final release)")


def check_no_signing_key_in_pr_workflows(workflows: dict[str, str], errors: list[str]) -> None:
    for name, text in workflows.items():
        if SIGNING_KEY_TOKEN in text:
            errors.append(f"{name}: PR workflows must not reference {SIGNING_KEY_TOKEN}")


def check_setup_integration_guardrails(content: str, errors: list[str]) -> None:
    if content.count("#[sqlx::test]") < 3:
        errors.append("setup_integration.rs: expected sqlx-isolated tests for all setup integration cases")

    forbid_pattern(content, r"#\[serial\(setup\)\]", "setup_integration.rs", errors)
    forbid_pattern(content, r"Config::default_for_test\(", "setup_integration.rs", errors)
    forbid_pattern(content, r"if\s+user_count\s*!=\s*0\s*\{\s*return\s*;", "setup_integration.rs", errors)
    forbid_pattern(content, r"user_count2\s*>=\s*1", "setup_integration.rs", errors)

    if not re.search(
        r'assert_eq!\(\s*user_count\s*,\s*0\s*,\s*"Should have 0 users initially"\s*\)',
        content,
        flags=re.MULTILINE,
    ):
        errors.append("setup_integration.rs: missing strict assert_eq!(user_count, 0, ...)")

    if not re.search(
        r'assert_eq!\(\s*user_count2\s*,\s*1\s*,\s*"Should have 1 user before second registration"\s*\)',
        content,
        flags=re.MULTILINE,
    ):
        errors.append("setup_integration.rs: missing strict assert_eq!(user_count2, 1, ...)")


def check_uploads_guardrails(content: str, errors: list[str]) -> None:
    if not re.search(
        r"async fn test_get_attachment_anti_enumeration_parity\(",
        content,
    ):
        errors.append("uploads_http.rs: missing required function test_get_attachment_anti_enumeration_parity")
    if not re.search(
        r'assert_eq!\(\s*resp\.status\(\)\s*,\s*403\s*,\s*"GET nonexistent attachment should return 403"\s*\)',
        content,
        flags=re.MULTILINE,
    ):
        errors.append("uploads_http.rs: missing strict 403 assertion for nonexistent attachment")

    forbid_pattern(
        content,
        r"resp\.status\(\)\s*==\s*403\s*\|\|\s*resp\.status\(\)\s*==\s*404",
        "uploads_http.rs",
        errors,
    )


def load_plist(path: Path, label: str, errors: list[str]) -> dict:
    text = read_text(path, errors)
    if not text:
        if path.exists():
            errors.append(f"{label}: file is empty")
        return {}
    try:
        data = plistlib.loads(text.encode("utf-8"))
    except Exception as exc:  # plistlib raises several unrelated exception types
        errors.append(f"{label}: not a valid plist: {exc}")
        return {}
    if not isinstance(data, dict):
        errors.append(f"{label}: top-level value must be a dict")
        return {}
    return data


def check_macos_bundle(errors: list[str], tauri_dir: Path = TAURI_DIR) -> None:
    """macOS kills the app on first mic/camera use without usage strings."""
    info = load_plist(tauri_dir / "Info.plist", "Info.plist", errors)
    if (tauri_dir / "Info.plist").exists():
        for key in MACOS_USAGE_KEYS:
            value = info.get(key)
            if not isinstance(value, str) or not value.strip():
                errors.append(f"Info.plist: {key} missing or not a non-empty string")

    conf_path = tauri_dir / "tauri.conf.json"
    conf_text = read_text(conf_path, errors)
    if not conf_text:
        return
    try:
        conf = json.loads(conf_text)
    except json.JSONDecodeError as exc:
        errors.append(f"tauri.conf.json: invalid JSON: {exc}")
        return
    entitlements = conf.get("bundle", {}).get("macOS", {}).get("entitlements")
    if not entitlements:
        errors.append("tauri.conf.json: bundle.macOS.entitlements not set")
        return
    ent_path = tauri_dir / entitlements
    if not ent_path.is_file():
        errors.append(f"tauri.conf.json: entitlements file not found: {entitlements}")
        return
    ent = load_plist(ent_path, entitlements, errors)
    for key in MACOS_ENTITLEMENT_KEYS:
        if ent.get(key) is not True:
            errors.append(f"{entitlements}: {key} not set to true")


def main() -> int:
    errors: list[str] = []

    action_text = read_text(NATIVE_DEPS_ACTION, errors)
    ci_text = read_text(CI_WORKFLOW, errors)
    tauri_text = read_text(TAURI_WORKFLOW, errors)
    release_text = read_text(RELEASE_WORKFLOW, errors)
    setup_integration_text = read_text(SETUP_INTEGRATION, errors)
    uploads_http_text = read_text(UPLOADS_HTTP, errors)

    if action_text:
        check_windows_libvpx_guardrails(action_text, errors)
    if ci_text and tauri_text and release_text:
        check_native_deps_usage(
            {"ci.yml": ci_text, "tauri-build.yml": tauri_text, "release.yml": release_text},
            errors,
        )
        check_no_signing_key_in_pr_workflows({"ci.yml": ci_text, "tauri-build.yml": tauri_text}, errors)
    if release_text:
        try:
            release = yaml.safe_load(release_text) or {}
        except yaml.YAMLError as exc:
            errors.append(f"release.yml: invalid YAML: {exc}")
        else:
            check_release_workflow(release, ROOT, errors)
            check_release_secret_scoping(release, errors)
            check_release_hygiene(release_text, release, errors)
            check_release_android(release, errors)
            check_release_publish_order(release, errors)
            check_server_image(release, "release.yml", errors)
    if ci_text:
        try:
            ci = yaml.safe_load(ci_text) or {}
        except yaml.YAMLError as exc:
            errors.append(f"ci.yml: invalid YAML: {exc}")
        else:
            check_server_image(ci, "ci.yml", errors)
        check_ci_never_tags_latest(ci_text, errors)
    config_text = read_text(CONFIG_RS, errors)
    env_doc_text = read_text(ENV_DOC, errors)
    if config_text and env_doc_text:
        check_env_doc(config_text, env_doc_text, errors)
    if setup_integration_text:
        check_setup_integration_guardrails(setup_integration_text, errors)
    if uploads_http_text:
        check_uploads_guardrails(uploads_http_text, errors)

    check_macos_bundle(errors)

    workflow_texts: dict[str, str] = {}
    workflow_docs: dict[str, dict] = {}
    for wf_path in sorted(WORKFLOW_DIR.glob("*.yml")):
        text = read_text(wf_path, errors)
        workflow_texts[wf_path.name] = text
        try:
            workflow_docs[wf_path.name] = yaml.safe_load(text) or {}
        except yaml.YAMLError as exc:
            errors.append(f"{wf_path.name}: invalid YAML: {exc}")
    check_workflow_permissions(workflow_docs, errors)
    check_actions_pinned(workflow_texts, errors)
    for gated in ("ci.yml", "tauri-build.yml"):
        if gated in workflow_docs:
            check_no_advisory_gates(workflow_docs[gated], errors)

    if errors:
        print("CI guardrails check failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print("CI guardrails check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
