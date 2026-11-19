#!/usr/bin/env bash
# Verify an Android APK: signature (v2), versionName/versionCode, signer cert, and optionally the
# verified setup App Link.
# Usage: android_verify_apk.sh <apk> <versionName> <versionCode> [--expect-unsigned] [--app-link-host <host>]
#   --app-link-host: the APK's manifest must carry, on MainActivity, an autoVerify intent filter for
#   exactly https://<host>/setup (the build flag -PkaikuAppLinkHosts); a release that lost it fails.
set -eu

usage() {
  echo "usage: $0 <apk> <versionName> <versionCode> [--expect-unsigned] [--app-link-host <host>]" >&2
  exit 2
}

fail() {
  echo "android_verify_apk: FAILED check: $1" >&2
  exit 1
}

[ "$#" -ge 3 ] || usage
APK=$1
NAME=$2
CODE=$3
shift 3
EXPECT_UNSIGNED=0
APP_LINK_HOST=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --expect-unsigned) EXPECT_UNSIGNED=1; shift ;;
    --app-link-host) [ "$#" -ge 2 ] && [ -n "$2" ] || usage; APP_LINK_HOST=$2; shift 2 ;;
    *) usage ;;
  esac
done

[ -f "$APK" ] || fail "apk exists ($APK)"
[ -n "${ANDROID_HOME:-}" ] || fail "ANDROID_HOME is set"

BT=$(ls -d "$ANDROID_HOME"/build-tools/*/ 2>/dev/null | sort -V | tail -n 1)
[ -n "$BT" ] || fail "build-tools found under \$ANDROID_HOME/build-tools"
APKSIGNER="${BT}apksigner"
AAPT2="${BT}aapt2"

if [ "$EXPECT_UNSIGNED" -eq 1 ]; then
  if "$APKSIGNER" verify "$APK" >/dev/null 2>&1; then
    fail "apk is unsigned (but apksigner verification succeeded)"
  fi
  echo "OK: apk is unsigned as expected"
else
  OUT=$("$APKSIGNER" verify --verbose --print-certs "$APK" 2>&1) \
    || fail "apksigner verify (exit non-zero)"
  echo "$OUT" | grep -qF "Verified using v2 scheme (APK Signature Scheme v2): true" \
    || fail "v2 signature scheme verified"
  if echo "$OUT" | grep -qF "CN=Android Debug"; then
    fail "signer is not the Android debug key"
  fi
  echo "$OUT" | grep "certificate SHA-256 digest" || true
fi

BADGING=$("$AAPT2" dump badging "$APK" 2>&1) || fail "aapt2 dump badging"
echo "$BADGING" | grep -qF "versionCode='$CODE'" || fail "versionCode='$CODE'"
echo "$BADGING" | grep -qF "versionName='$NAME'" || fail "versionName='$NAME'"
echo "OK: $APK versionName=$NAME versionCode=$CODE"

if [ -n "$APP_LINK_HOST" ]; then
  TREE=$("$AAPT2" dump xmltree --file AndroidManifest.xml "$APK" 2>&1) || fail "aapt2 dump xmltree AndroidManifest.xml"
  echo "$TREE" | python3 -c '
import re, sys

host = sys.argv[1]
# aapt2 xmltree: "E: <tag>" elements and "A: <ns>:<name>(<id>)=<value>" attributes, nested by indent.
nodes, stack = [], []
for line in sys.stdin:
    m = re.match(r"^(\s*)(E|A): (.*)$", line.rstrip("\n"))
    if not m:
        continue
    depth, kind, rest = len(m.group(1)), m.group(2), m.group(3)
    if kind == "E":
        node = {"tag": rest.split(" ")[0], "attrs": {}, "children": [], "depth": depth}
        while stack and stack[-1]["depth"] >= depth:
            stack.pop()
        (stack[-1]["children"] if stack else nodes).append(node)
        stack.append(node)
    elif stack:
        a = re.match(r"^(?:\S+:)?([A-Za-z]+)\([^)]*\)=(.*)$", rest) or re.match(r"^(?:\S+:)?([A-Za-z]+)=(.*)$", rest)
        if a:
            v = a.group(2)
            raw = re.search(r"\(Raw: \"([^\"]*)\"\)", v)
            v = raw.group(1) if raw else v.strip().strip("\"")
            stack[-1]["attrs"][a.group(1)] = v

def walk(ns, tag):
    for n in ns:
        if n["tag"] == tag:
            yield n
        yield from walk(n["children"], tag)

found = False
for act in walk(nodes, "activity"):
    if not act["attrs"].get("name", "").endswith(".ui.MainActivity"):
        continue
    for f in walk(act["children"], "intent-filter"):
        data = [d["attrs"] for d in walk(f["children"], "data")]
        schemes = {d["scheme"] for d in data if "scheme" in d}
        hosts = {d["host"] for d in data if "host" in d}
        paths = {d[k] for d in data for k in ("path", "pathPrefix", "pathPattern") if k in d}
        verify = f["attrs"].get("autoVerify", "").lower() in ("true", "0xffffffff", "-1")
        if schemes == {"https"} and hosts == {host} and paths == {"/setup"} and verify:
            found = True
sys.exit(0 if found else 1)
' "$APP_LINK_HOST" || fail "verified App Link filter for https://$APP_LINK_HOST/setup on MainActivity"
  echo "OK: verified App Link https://$APP_LINK_HOST/setup"
fi
