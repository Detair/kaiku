#!/usr/bin/env python3
"""Write a Tauri config overlay that signs Windows bundles via Trusted Signing.

Usage:
  write_winsign_config.py --output <file>

Reads AZURE_TRUSTED_SIGNING_ENDPOINT/ACCOUNT/PROFILE (repo variables, not
secrets) and writes {"bundle":{"windows":{"signCommand": ...}}} for
`tauri build --config <file>`. Exits 1 naming any missing variable.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

VARS = ("AZURE_TRUSTED_SIGNING_ENDPOINT", "AZURE_TRUSTED_SIGNING_ACCOUNT", "AZURE_TRUSTED_SIGNING_PROFILE")


def sign_command(endpoint: str, account: str, profile: str) -> str:
    return f"trusted-signing-cli -e {endpoint} -a {account} -c {profile} -d Kaiku %1"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)

    missing = [name for name in VARS if not os.environ.get(name)]
    if missing:
        print(f"::error::Missing Windows signing variables: {', '.join(missing)}")
        return 1
    cmd = sign_command(*(os.environ[name] for name in VARS))
    args.output.write_text(json.dumps({"bundle": {"windows": {"signCommand": cmd}}}), encoding="utf-8")
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
