#!/usr/bin/env python3
"""Decide whether a final release may move the `latest` image tag.

Usage:
  latest_tag_decision.py classify <pull-stderr-file>
      prints `absent` (registry says the tag/repo does not exist) or `error`.
  latest_tag_decision.py decide --new <ver> (--absent | --current <label>)
      prints `push` or `skip`; a reason goes to stderr.

Exit codes: 0 ok, 2 invalid version or usage.
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_version import VersionError, compare_versions, parse_version  # noqa: E402

# Only the registry's own "does not exist" answers count as absent. Generic
# "not found" (DNS, missing binary, auth) must stay an error.
_ABSENT_RE = re.compile(r"manifest unknown|name unknown", re.IGNORECASE)


def classify_pull_error(stderr: str) -> str:
    return "absent" if _ABSENT_RE.search(stderr) else "error"


def decide(new: str, current: str | None) -> tuple[str, str]:
    """Return (push|skip, reason). ``current`` None means latest is absent."""
    new_v = parse_version(new)
    if current is None:
        return "push", "latest does not exist yet"
    try:
        cur = parse_version(current)
    except VersionError:
        return "skip", f"current latest has unparseable version label {current!r}"
    if compare_versions(new_v, cur) < 0:
        return "skip", f"latest is at {cur.semver}, newer than {new_v.semver}"
    return "push", f"{new} is not older than current latest {cur.semver}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("classify").add_argument("file")
    d = sub.add_parser("decide")
    d.add_argument("--new", required=True)
    g = d.add_mutually_exclusive_group(required=True)
    g.add_argument("--absent", action="store_true")
    g.add_argument("--current")
    args = parser.parse_args(argv)
    if args.command == "classify":
        print(classify_pull_error(Path(args.file).read_text(encoding="utf-8", errors="replace")))
        return 0
    try:
        action, reason = decide(args.new, None if args.absent else args.current)
    except VersionError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print(reason, file=sys.stderr)
    print(action)
    return 0


if __name__ == "__main__":
    sys.exit(main())
