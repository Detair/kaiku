#!/usr/bin/env python3
"""Write the Tauri updater manifest (latest.json) for a GitHub release.

Usage:
  generate_updater_manifest.py --assets-dir <dir> --tag <tag> --repo <owner/name>
                               --mode {publish,dry-run} --output <file>

Each platform needs exactly one matching asset with a sibling `.sig` file.
In publish mode any missing platform is an error (exit 1, nothing written);
in dry-run it is a warning and the manifest (if any platform was found) is
written with the platforms that are present.
"""
from __future__ import annotations

import argparse
import datetime
import json
import sys
from pathlib import Path

# Platform key -> glob of the updater bundle in the flat release-assets dir.
PLATFORMS: dict[str, str] = {
    "linux-x86_64": "*.AppImage",
    "windows-x86_64": "*-setup.exe",
    "darwin-x86_64": "*x86_64.app.tar.gz",
    "darwin-aarch64": "*aarch64.app.tar.gz",
}


def _signature(asset: Path) -> str:
    """Contents of the sibling .sig, or '' if it is absent or blank."""
    sig = asset.with_name(asset.name + ".sig")
    return sig.read_text(encoding="utf-8").strip() if sig.is_file() else ""


def find_platforms(assets_dir: Path, tag: str, repo: str) -> tuple[dict[str, dict[str, str]], list[str]]:
    base = f"https://github.com/{repo}/releases/download/{tag}"
    platforms: dict[str, dict[str, str]] = {}
    problems: list[str] = []
    for key, pattern in PLATFORMS.items():
        signed = [p for p in sorted(assets_dir.glob(pattern)) if _signature(p)]
        if not signed:
            problems.append(f"{key}: no signed asset matching {pattern} (needs a non-empty sibling .sig)")
            continue
        if len(signed) > 1:
            names = ", ".join(p.name for p in signed)
            problems.append(f"{key}: ambiguous, several signed assets match {pattern}: {names}")
            continue
        asset = signed[0]
        signature = _signature(asset)
        platforms[key] = {"signature": signature, "url": f"{base}/{asset.name}"}
    return platforms, problems


def build_manifest(tag: str, repo: str, platforms: dict[str, dict[str, str]]) -> dict:
    return {
        "version": tag.removeprefix("v"),
        "notes": f"See https://github.com/{repo}/releases/tag/{tag}",
        "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "platforms": platforms,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--assets-dir", required=True, type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--mode", required=True, choices=("publish", "dry-run"))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)

    platforms, problems = find_platforms(args.assets_dir, args.tag, args.repo)
    level = "error" if args.mode == "publish" else "warning"
    for problem in problems:
        print(f"::{level}::updater manifest: {problem}", file=sys.stderr)
    if problems and args.mode == "publish":
        print("::error::updater manifest incomplete; refusing to publish (check TAURI_SIGNING_PRIVATE_KEY)", file=sys.stderr)
        return 1
    if not platforms:
        print("::warning::updater manifest: no signed platforms found, latest.json not written (dry run)", file=sys.stderr)
        return 0

    manifest = build_manifest(args.tag, args.repo, platforms)
    args.output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
