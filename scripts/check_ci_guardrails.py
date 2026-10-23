#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"
TAURI_WORKFLOW = ROOT / ".github/workflows/tauri-build.yml"
SETUP_INTEGRATION = ROOT / "server/tests/integration/setup_integration.rs"
UPLOADS_HTTP = ROOT / "server/tests/integration/uploads_http.rs"
TAURI_DIR = ROOT / "client/src-tauri"
TAURI_CONF = TAURI_DIR / "tauri.conf.json"
INFO_PLIST = TAURI_DIR / "Info.plist"
MACOS_USAGE_KEYS = ["NSMicrophoneUsageDescription", "NSCameraUsageDescription"]

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


def check_windows_libvpx_guardrails(ci_text: str, tauri_text: str, errors: list[str]) -> None:
    for token in WINDOWS_LIBVPX_TOKENS:
        require_token(ci_text, token, "ci.yml", errors)
        require_token(tauri_text, token, "tauri-build.yml", errors)

    forbid_pattern(ci_text, r"PKG_CONFIG=.*pkg-config|PKG_CONFIG=.*pkgconf", "ci.yml", errors)


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


def check_macos_bundle(errors: list[str]) -> None:
    """macOS kills the app on first mic/camera use without usage strings."""
    info = read_text(INFO_PLIST, errors)
    for key in MACOS_USAGE_KEYS:
        if info and not re.search(
            rf"<key>{key}</key>\s*<string>\s*\S[^<]*</string>", info
        ):
            errors.append(f"Info.plist: {key} missing or empty")

    conf_text = read_text(TAURI_CONF, errors)
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
    elif not (TAURI_DIR / entitlements).is_file():
        errors.append(f"tauri.conf.json: entitlements file not found: {entitlements}")
    else:
        ent = (TAURI_DIR / entitlements).read_text(encoding="utf-8")
        for key in ("com.apple.security.device.audio-input", "com.apple.security.device.camera"):
            if not re.search(rf"<key>{re.escape(key)}</key>\s*<true\s*/>", ent):
                errors.append(f"{entitlements}: {key} not set to true")


def main() -> int:
    errors: list[str] = []

    ci_text = read_text(CI_WORKFLOW, errors)
    tauri_text = read_text(TAURI_WORKFLOW, errors)
    setup_integration_text = read_text(SETUP_INTEGRATION, errors)
    uploads_http_text = read_text(UPLOADS_HTTP, errors)

    if ci_text and tauri_text:
        check_windows_libvpx_guardrails(ci_text, tauri_text, errors)
    if setup_integration_text:
        check_setup_integration_guardrails(setup_integration_text, errors)
    if uploads_http_text:
        check_uploads_guardrails(uploads_http_text, errors)

    check_macos_bundle(errors)

    if errors:
        print("CI guardrails check failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print("CI guardrails check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
