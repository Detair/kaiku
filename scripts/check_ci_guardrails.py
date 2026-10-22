#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Any

import yaml


ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"
TAURI_WORKFLOW = ROOT / ".github/workflows/tauri-build.yml"
RELEASE_WORKFLOW = ROOT / ".github/workflows/release.yml"
NATIVE_DEPS_ACTION = ROOT / ".github/actions/setup-native-deps/action.yml"
NATIVE_DEPS_ACTION_REF = "./.github/actions/setup-native-deps"
SETUP_INTEGRATION = ROOT / "server/tests/integration/setup_integration.rs"
UPLOADS_HTTP = ROOT / "server/tests/integration/uploads_http.rs"

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
    if setup_integration_text:
        check_setup_integration_guardrails(setup_integration_text, errors)
    if uploads_http_text:
        check_uploads_guardrails(uploads_http_text, errors)

    if errors:
        print("CI guardrails check failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print("CI guardrails check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
