#!/usr/bin/env python3
"""Gate a release step on secrets being present, without printing them.

Usage:
  require_secrets.py --mode {publish,dry-run} --output <name> [--step <label>] VAR [VAR...]

Reads each VAR from the environment; an empty string counts as missing.
  all present          -> writes <name>=true to $GITHUB_OUTPUT, exit 0
  missing, dry-run     -> writes <name>=false, prints ::notice::, exit 0
  missing, publish     -> prints ::error::, exit 1
Only variable names are ever printed, never values.
"""
from __future__ import annotations

import argparse
import os
import sys


def write_output(name: str, value: str) -> None:
    path = os.environ.get("GITHUB_OUTPUT")
    line = f"{name}={value}\n"
    if path:
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(line)
    else:
        sys.stdout.write(line)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", required=True, choices=("publish", "dry-run"))
    parser.add_argument("--output", required=True, help="GITHUB_OUTPUT key to set to true/false")
    parser.add_argument("--step", help="Human label of the gated step (default: the output name)")
    parser.add_argument("names", nargs="+", metavar="VAR")
    args = parser.parse_args(argv)

    missing = [name for name in args.names if not os.environ.get(name)]
    if not missing:
        write_output(args.output, "true")
        return 0

    names = ", ".join(missing)
    if args.mode == "publish":
        print(f"::error::Missing required secrets in Environment 'release': {names}")
        return 1

    write_output(args.output, "false")
    print(f"::notice::{names} not set — {args.step or args.output} skipped (dry run)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
