#!/usr/bin/env python3
"""Single source of truth for the Kaiku release version.

The release version lives in five places that must always agree:
the Tauri config, the root Cargo workspace, the client package.json and the
Android version.properties (versionName + derived versionCode). Cargo.lock
entries of the workspace crates are rewritten alongside on bump.

Usage:
  release_version.py parse <tag>          KEY=value lines for $GITHUB_OUTPUT
  release_version.py bump <ver>           rewrite all version lines to <ver>
  release_version.py check [--tag <tag>]  verify all files agree (and match tag)
  release_version.py android-code <ver>   print the Android versionCode

Exit codes: 0 ok, 1 inconsistent, 2 invalid version or usage.
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

_VERSION_RE = re.compile(r"^v?(0|[1-9]\d?)\.(0|[1-9]\d?)\.(0|[1-9]\d?)(?:-rc\.([1-9]\d?))?$")

# Final releases use 99 in the rc slot so they sort after every rc (1..98).
_FINAL_RC_SLOT = 99

TAURI_CONF = "client/src-tauri/tauri.conf.json"
CARGO_TOML = "Cargo.toml"
PACKAGE_JSON = "client/package.json"
VERSION_PROPERTIES = "mobile/android/version.properties"
CARGO_LOCK = "Cargo.lock"

LABEL_VERSION_NAME = f"{VERSION_PROPERTIES}:versionName"
LABEL_VERSION_CODE = f"{VERSION_PROPERTIES}:versionCode"

VERSION_FILES: dict[str, str] = {
    TAURI_CONF: 'top-level JSON line `  "version": "<semver>",`',
    CARGO_TOML: '`version = "<semver>"` inside [workspace.package]',
    PACKAGE_JSON: 'top-level JSON line `  "version": "<semver>",`',
    LABEL_VERSION_NAME: "`versionName=<semver>`",
    LABEL_VERSION_CODE: "`versionCode=<android_version_code>`",
}

WORKSPACE_CRATES = ("vc-server", "vc-client", "vc-common", "vc-crypto", "kaiku-vp8-decoder")

_JSON_VERSION_RE = re.compile(r'^(  "version": ")([^"]*)(",)$')
_TOML_VERSION_RE = re.compile(r'^(version = ")([^"]*)(")$')


class VersionError(ValueError):
    """Invalid version string or unreadable/malformed version file."""


@dataclass(frozen=True)
class ReleaseVersion:
    major: int
    minor: int
    patch: int
    rc: int | None

    @property
    def semver(self) -> str:
        base = f"{self.major}.{self.minor}.{self.patch}"
        return base if self.rc is None else f"{base}-rc.{self.rc}"

    @property
    def tag(self) -> str:
        return f"v{self.semver}"

    @property
    def is_prerelease(self) -> bool:
        return self.rc is not None

    @property
    def android_version_code(self) -> int:
        rc_slot = _FINAL_RC_SLOT if self.rc is None else self.rc
        return self.major * 1_000_000 + self.minor * 10_000 + self.patch * 100 + rc_slot


def parse_version(text: str) -> ReleaseVersion:
    match = _VERSION_RE.fullmatch(text)
    if match is None:
        raise VersionError(f"invalid version {text!r}: expected [v]X.Y.Z or [v]X.Y.Z-rc.N (0-99, rc 1-98)")
    major, minor, patch, rc = match.groups()
    rc_num = None if rc is None else int(rc)
    if rc_num == _FINAL_RC_SLOT:
        raise VersionError(f"invalid version {text!r}: rc.99 is reserved for the final release")
    return ReleaseVersion(int(major), int(minor), int(patch), rc_num)


# ---------------------------------------------------------------------------
# Line-level helpers (never round-trip through a parser, keep formatting).
# ---------------------------------------------------------------------------


def _read_lines(root: Path, rel: str) -> list[str]:
    path = root / rel
    if not path.exists():
        raise VersionError(f"{rel}: file missing")
    return path.read_text(encoding="utf-8").splitlines(keepends=True)


def _split_eol(line: str) -> tuple[str, str]:
    body = line.rstrip("\r\n")
    return body, line[len(body):]


def _json_version_index(lines: list[str], rel: str) -> int:
    hits = [i for i, line in enumerate(lines) if _JSON_VERSION_RE.match(_split_eol(line)[0])]
    if len(hits) != 1:
        raise VersionError(f'{rel}: expected exactly one top-level `  "version": "...",` line, found {len(hits)}')
    return hits[0]


def _cargo_toml_version_index(lines: list[str], rel: str) -> int:
    in_section = False
    for i, line in enumerate(lines):
        body = _split_eol(line)[0]
        if body.lstrip().startswith("["):
            in_section = body.strip() == "[workspace.package]"
            continue
        if in_section and _TOML_VERSION_RE.match(body):
            return i
    raise VersionError(f"{rel}: no `version = \"...\"` line in [workspace.package]")


def _properties_index(lines: list[str], rel: str, key: str) -> int:
    hits = [i for i, line in enumerate(lines) if _split_eol(line)[0].startswith(f"{key}=")]
    if len(hits) != 1:
        raise VersionError(f"{rel}: expected exactly one `{key}=` line, found {len(hits)}")
    return hits[0]


def _cargo_lock_indices(lines: list[str], rel: str) -> list[int]:
    indices = []
    for crate in WORKSPACE_CRATES:
        hits = [
            i + 1
            for i, line in enumerate(lines[:-1])
            if _split_eol(line)[0] == f'name = "{crate}"' and _TOML_VERSION_RE.match(_split_eol(lines[i + 1])[0])
        ]
        if len(hits) != 1:
            raise VersionError(f"{rel}: expected one `name = \"{crate}\"` + version entry, found {len(hits)}")
        indices.append(hits[0])
    return indices


def _value(line: str, pattern: re.Pattern[str]) -> str:
    match = pattern.match(_split_eol(line)[0])
    assert match is not None
    return match.group(2)


def _replace(line: str, pattern: re.Pattern[str], value: str) -> str:
    body, eol = _split_eol(line)
    return pattern.sub(lambda m: f"{m.group(1)}{value}{m.group(3)}", body) + eol


def _prop_value(line: str) -> str:
    return _split_eol(line)[0].split("=", 1)[1].strip()


def _set_prop(line: str, key: str, value: str) -> str:
    return f"{key}={value}{_split_eol(line)[1]}"


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def read_versions(root: Path) -> dict[str, str]:
    versions: dict[str, str] = {}
    for rel in (TAURI_CONF, PACKAGE_JSON):
        lines = _read_lines(root, rel)
        versions[rel] = _value(lines[_json_version_index(lines, rel)], _JSON_VERSION_RE)
    lines = _read_lines(root, CARGO_TOML)
    versions[CARGO_TOML] = _value(lines[_cargo_toml_version_index(lines, CARGO_TOML)], _TOML_VERSION_RE)
    lines = _read_lines(root, VERSION_PROPERTIES)
    versions[LABEL_VERSION_NAME] = _prop_value(lines[_properties_index(lines, VERSION_PROPERTIES, "versionName")])
    versions[LABEL_VERSION_CODE] = _prop_value(lines[_properties_index(lines, VERSION_PROPERTIES, "versionCode")])
    return {label: versions[label] for label in VERSION_FILES}


def write_version(root: Path, v: ReleaseVersion) -> list[Path]:
    """Rewrite only the version lines; return the files whose content changed."""
    planned: dict[str, list[str]] = {}

    for rel in (TAURI_CONF, PACKAGE_JSON):
        lines = _read_lines(root, rel)
        idx = _json_version_index(lines, rel)
        lines[idx] = _replace(lines[idx], _JSON_VERSION_RE, v.semver)
        planned[rel] = lines

    lines = _read_lines(root, CARGO_TOML)
    idx = _cargo_toml_version_index(lines, CARGO_TOML)
    lines[idx] = _replace(lines[idx], _TOML_VERSION_RE, v.semver)
    planned[CARGO_TOML] = lines

    lines = _read_lines(root, VERSION_PROPERTIES)
    for key, value in (("versionName", v.semver), ("versionCode", str(v.android_version_code))):
        idx = _properties_index(lines, VERSION_PROPERTIES, key)
        lines[idx] = _set_prop(lines[idx], key, value)
    planned[VERSION_PROPERTIES] = lines

    lines = _read_lines(root, CARGO_LOCK)
    for idx in _cargo_lock_indices(lines, CARGO_LOCK):
        lines[idx] = _replace(lines[idx], _TOML_VERSION_RE, v.semver)
    planned[CARGO_LOCK] = lines

    # All files validated before any write, so a malformed file aborts cleanly.
    changed: list[Path] = []
    for rel, new_lines in planned.items():
        path = root / rel
        new_text = "".join(new_lines)
        if path.read_text(encoding="utf-8") != new_text:
            path.write_text(new_text, encoding="utf-8")
            changed.append(path)
    return changed


def check_consistency(root: Path, expected: ReleaseVersion | None) -> list[str]:
    """Return one error string per inconsistent file (empty list = consistent).

    Without ``expected`` the root Cargo.toml workspace version is the reference.
    """
    try:
        versions = read_versions(root)
    except VersionError as exc:
        return [str(exc)]

    if expected is None:
        try:
            expected = parse_version(versions[CARGO_TOML])
        except VersionError as exc:
            return [f"{CARGO_TOML}: {exc}"]

    errors = []
    for label, actual in versions.items():
        want = str(expected.android_version_code) if label == LABEL_VERSION_CODE else expected.semver
        if actual != want:
            errors.append(f"{label}: found {actual}, expected {want}")
    return errors


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


class _Parser(argparse.ArgumentParser):
    def error(self, message: str):  # usage errors -> exit 2 (argparse default, made explicit)
        self.print_usage(sys.stderr)
        self.exit(2, f"{self.prog}: error: {message}\n")


def main(argv: list[str] | None = None) -> int:
    parser = _Parser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_Parser)
    sub.add_parser("parse", help="print KEY=value lines for a tag").add_argument("tag")
    sub.add_parser("bump", help="rewrite all version files").add_argument("version")
    check = sub.add_parser("check", help="verify version files agree")
    check.add_argument("--tag", help="also require the files to match this tag")
    sub.add_parser("android-code", help="print Android versionCode").add_argument("version")
    args = parser.parse_args(argv)

    try:
        if args.command == "parse":
            v = parse_version(args.tag)
            print(f"version={v.semver}")
            print(f"tag={v.tag}")
            print(f"is_prerelease={'true' if v.is_prerelease else 'false'}")
            print(f"android_version_code={v.android_version_code}")
            return 0
        if args.command == "android-code":
            print(parse_version(args.version).android_version_code)
            return 0
        if args.command == "bump":
            v = parse_version(args.version)
            changed = write_version(ROOT, v)
            for path in changed:
                print(f"updated {path.relative_to(ROOT)}")
            if not changed:
                print(f"all version files already at {v.semver}")
            return 0
        # check
        expected = parse_version(args.tag) if args.tag else None
        errors = check_consistency(ROOT, expected)
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        if errors:
            return 1
        print("version files consistent")
        return 0
    except VersionError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
