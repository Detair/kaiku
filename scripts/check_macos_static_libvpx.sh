#!/usr/bin/env bash
# Check that the macOS desktop app links libvpx statically.
#
# The screen share encoder/decoder use libvpx. A dynamically linked libvpx
# (e.g. /opt/homebrew/opt/libvpx/lib/libvpx.*.dylib) works on the build
# machine but fails to start on every Mac without Homebrew's libvpx.
# `otool -L` lists the dynamic libraries of the binary: libvpx must not be
# among them.
#
# Usage: scripts/check_macos_static_libvpx.sh [path/to/Kaiku.app | path/to/binary]
# Default: the first .app under target/release/bundle/macos/.
set -euo pipefail

target="${1:-}"
if [[ -z "$target" ]]; then
  target="$(ls -d target/release/bundle/macos/*.app 2>/dev/null | head -n 1 || true)"
fi
if [[ -z "$target" || ! -e "$target" ]]; then
  echo "ERROR: no app bundle or binary found (got '${target}')" >&2
  exit 2
fi

if [[ -d "$target" ]]; then
  binaries=("$target"/Contents/MacOS/*)
else
  binaries=("$target")
fi

status=0
checked=0
for bin in "${binaries[@]}"; do
  [[ -f "$bin" ]] || continue
  checked=$((checked + 1))
  echo "== otool -L $bin"
  libs="$(otool -L "$bin")"
  echo "$libs"
  if grep -qi 'libvpx' <<<"$libs"; then
    echo "ERROR: $bin links libvpx dynamically; it must be linked statically." >&2
    status=1
  fi
done
# An empty or mispackaged bundle must not pass: nothing checked is a failure.
if [[ $checked -eq 0 ]]; then
  echo "ERROR: no binary found to check in '${target}'" >&2
  exit 2
fi
if [[ $status -eq 0 ]]; then
  echo "OK: libvpx is not a dynamic dependency."
fi
exit $status
