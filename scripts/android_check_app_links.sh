#!/usr/bin/env bash
# Checks the merged debug manifest for the setup App Links (C29, L2-A15).
# Run after: (cd mobile/android && ./gradlew :app:processDebugMainManifest [-PkaikuAppLinkHosts=<host>])
# Usage: android_check_app_links.sh [<host>[,<host>...]]   (as -PkaikuAppLinkHosts)
#   with hosts: one autoVerify https filter for each <host>/setup, and the kaiku://setup filter
#   without:    no https /setup filter at all, and the kaiku://setup filter
# Always: no other kaiku:// link than auth (OIDC callback) and setup, never kaiku://login (C27).
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
manifest, host_arg = sys.argv[1], sys.argv[2]
expected = sorted(h.strip().lower() for h in host_arg.split(",") if h.strip())
root = ET.parse(manifest).getroot()


def attrs(f, name):
    # <data> attributes of one filter combine across sibling <data> elements (Android merges them).
    return {d.get(A + name) for d in f.findall("data") if d.get(A + name)}


setup_https, setup_kaiku, unexpected = [], 0, []
for act in root.iter("activity"):
    for f in act.findall("intent-filter"):
        schemes, hosts = attrs(f, "scheme"), attrs(f, "host")
        paths = attrs(f, "path") | attrs(f, "pathPrefix") | attrs(f, "pathPattern")
        if "kaiku" in schemes:
            # Only the OIDC callback and setup links: a sign-in is a scan, never a link (C27).
            if hosts == {"setup"}:
                setup_kaiku += 1
            elif hosts != {"auth"}:
                unexpected.append(("kaiku", sorted(hosts), sorted(paths)))
        if "https" in schemes:
            if paths == {"/setup"}:
                setup_https += [(h, f.get(A + "autoVerify")) for h in sorted(hosts)]
            elif paths != {"/auth/callback"}:
                unexpected.append(("https", sorted(hosts), sorted(paths)))
if unexpected:
    sys.exit("android_check_app_links: FAILED check: unexpected link filters %s" % unexpected)
if setup_kaiku != 1:
    sys.exit("android_check_app_links: FAILED check: one kaiku://setup filter (found %d)" % setup_kaiku)
if expected:
    if sorted(setup_https) != [(h, "true") for h in expected]:
        sys.exit("android_check_app_links: FAILED check: autoVerify /setup filters for %s (found %s)" % (expected, setup_https))
elif setup_https:
    sys.exit("android_check_app_links: FAILED check: no /setup App Link without kaikuAppLinkHosts (found %s)" % setup_https)
print("ok")
PY
