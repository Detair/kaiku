#!/usr/bin/env bash
# Checks the merged debug manifest for the setup App Links (C29, L2-A15).
# Run after: (cd mobile/android && ./gradlew :app:processDebugMainManifest [-PkaikuAppLinkHosts=<host>])
# Usage: android_check_app_links.sh [<host>]
#   with <host>: exactly one autoVerify https filter for <host>/setup, and the kaiku://setup filter
#   without:     no https /setup filter at all, and the kaiku://setup filter
set -eu

fail() {
  echo "android_check_app_links: FAILED check: $1" >&2
  exit 1
}

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MANIFEST=$(ls "$ROOT"/mobile/android/app/build/intermediates/merged_manifest/debug/*/AndroidManifest.xml 2>/dev/null | head -n 1 || true)
[ -n "$MANIFEST" ] && [ -f "$MANIFEST" ] || fail "merged debug manifest exists (run processDebugMainManifest first)"

HOST=${1:-}
python3 - "$MANIFEST" "$HOST" <<'PY' || exit 1
import sys
import xml.etree.ElementTree as ET

A = "{http://schemas.android.com/apk/res/android}"
manifest, host = sys.argv[1], sys.argv[2]
root = ET.parse(manifest).getroot()
setup_https, setup_kaiku = [], 0
for act in root.iter("activity"):
    for f in act.findall("intent-filter"):
        for d in f.findall("data"):
            if d.get(A + "scheme") == "https" and d.get(A + "path") == "/setup":
                setup_https.append((d.get(A + "host"), f.get(A + "autoVerify")))
            if d.get(A + "scheme") == "kaiku" and d.get(A + "host") == "setup":
                setup_kaiku += 1
if setup_kaiku != 1:
    sys.exit("android_check_app_links: FAILED check: one kaiku://setup filter (found %d)" % setup_kaiku)
if host:
    if setup_https != [(host, "true")]:
        sys.exit("android_check_app_links: FAILED check: one autoVerify /setup filter for %s (found %s)" % (host, setup_https))
elif setup_https:
    sys.exit("android_check_app_links: FAILED check: no /setup App Link without kaikuAppLinkHosts (found %s)" % setup_https)
print("ok")
PY
