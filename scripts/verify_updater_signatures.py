#!/usr/bin/env python3
"""Verify every latest.json updater signature against the pubkey clients trust.

Usage:
  verify_updater_signatures.py --manifest <latest.json> --assets-dir <dir>
                               --mode {publish,dry-run}
                               [--config client/src-tauri/tauri.conf.json]
                               [--minisign minisign]

Installed clients only accept an update whose signature verifies against
`plugins.updater.pubkey` in tauri.conf.json. A TAURI_SIGNING_PRIVATE_KEY that
does not belong to that pubkey still produces a "complete" latest.json, so a
release would ship with updates nobody can install. This script catches that
before publishing.

Both the pubkey and every `.sig` are base64-encoded minisign files (Tauri's
format); verification itself is done by the `minisign` binary (ed25519 is
not in the Python stdlib).

Publish mode: any problem is an ::error:: naming the platform, exit 1.
Dry-run mode: nothing is verified, a ::notice:: says so, exit 0.
"""
from __future__ import annotations

import argparse
import base64
import binascii
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = ROOT / "client/src-tauri/tauri.conf.json"


class VerifyError(Exception):
    pass


def _decode_minisign_file(b64: str, what: str) -> str:
    try:
        text = base64.b64decode(b64.strip(), validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as exc:
        raise VerifyError(f"{what} is not base64-encoded minisign text: {exc}") from exc
    lines = text.splitlines()
    if len(lines) < 2 or not lines[0].startswith("untrusted comment:"):
        raise VerifyError(f"{what} does not decode to a minisign file (missing 'untrusted comment:' line)")
    return text if text.endswith("\n") else text + "\n"


def _key_id(minisign_text: str, what: str) -> str:
    """Key id as minisign prints it (little-endian hex of bytes 2..10 of line 2)."""
    try:
        raw = base64.b64decode(minisign_text.splitlines()[1].strip(), validate=True)
    except binascii.Error as exc:
        raise VerifyError(f"{what}: malformed key/signature line: {exc}") from exc
    if len(raw) < 10:
        raise VerifyError(f"{what}: key/signature line too short")
    return raw[2:10][::-1].hex().upper()


def load_pubkey(config: Path) -> str:
    """Decoded minisign public key file from plugins.updater.pubkey."""
    try:
        data = json.loads(config.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise VerifyError(f"cannot read {config}: {exc}") from exc
    pubkey = ((data.get("plugins") or {}).get("updater") or {}).get("pubkey")
    if not isinstance(pubkey, str) or not pubkey.strip():
        raise VerifyError(f"{config}: plugins.updater.pubkey is missing or empty")
    return _decode_minisign_file(pubkey, f"{config.name} plugins.updater.pubkey")


def verify(manifest: Path, assets_dir: Path, config: Path, minisign: str) -> list[str]:
    """Return one problem per platform that does not verify (empty = all good)."""
    pubkey = load_pubkey(config)
    pub_id = _key_id(pubkey, "pubkey")
    binary = shutil.which(minisign)
    if binary is None:
        raise VerifyError(f"'{minisign}' not found; install it (apt-get install minisign)")
    try:
        platforms = json.loads(manifest.read_text(encoding="utf-8")).get("platforms") or {}
    except (OSError, json.JSONDecodeError) as exc:
        raise VerifyError(f"cannot read {manifest}: {exc}") from exc
    if not platforms:
        raise VerifyError(f"{manifest}: no platforms to verify")

    problems: list[str] = []
    with tempfile.TemporaryDirectory() as tmp:
        pub_file = Path(tmp) / "updater.pub"
        pub_file.write_text(pubkey, encoding="utf-8")
        for key, entry in sorted(platforms.items()):
            name = Path(unquote(urlparse(str(entry.get("url", ""))).path)).name
            asset = assets_dir / name
            if not name or not asset.is_file():
                problems.append(f"{key}: asset {name or '(no url)'} not found in {assets_dir}")
                continue
            try:
                sig = _decode_minisign_file(str(entry.get("signature", "")), f"{key} signature")
                sig_id = _key_id(sig, f"{key} signature")
            except VerifyError as exc:
                problems.append(str(exc))
                continue
            if sig_id != pub_id:
                problems.append(
                    f"{key}: {name} is signed with key {sig_id}, but tauri.conf.json "
                    f"plugins.updater.pubkey is key {pub_id} (wrong TAURI_SIGNING_PRIVATE_KEY?)"
                )
                continue
            sig_file = Path(tmp) / f"{key}.minisig"
            sig_file.write_text(sig, encoding="utf-8")
            result = subprocess.run(
                [binary, "-V", "-q", "-p", str(pub_file), "-m", str(asset), "-x", str(sig_file)],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout).strip().replace("\n", " ")
                problems.append(f"{key}: {name} signature does not verify against plugins.updater.pubkey ({detail})")
            else:
                print(f"{key}: {name} signature OK (key {pub_id})")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--assets-dir", required=True, type=Path)
    parser.add_argument("--mode", required=True, choices=("publish", "dry-run"))
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--minisign", default="minisign")
    args = parser.parse_args(argv)

    if args.mode == "dry-run":
        print("::notice::updater signature verification skipped (dry run: nothing is signed)")
        return 0
    try:
        problems = verify(args.manifest, args.assets_dir, args.config, args.minisign)
    except VerifyError as exc:
        print(f"::error::updater signature check: {exc}", file=sys.stderr)
        return 1
    for problem in problems:
        print(f"::error::updater signature check: {problem}", file=sys.stderr)
    if problems:
        print("::error::refusing to publish latest.json: clients would reject these updates", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
