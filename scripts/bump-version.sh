#!/bin/sh
# Bump the Kaiku release version in every version file at once.
# Usage: scripts/bump-version.sh X.Y.Z[-rc.N]
# Prerequisites: python3 (stdlib only). Run from anywhere inside the repo.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 X.Y.Z[-rc.N]" >&2
    exit 2
fi

cd "$(dirname "$0")/.."
python3 scripts/release_version.py bump "$1"

cat <<NEXT

Next steps:
  1. git commit -am "chore(infra): bump version to $1"
  2. Open a PR and get it merged.
  3. After merge, tag the merge commit (see the release runbook).
NEXT
