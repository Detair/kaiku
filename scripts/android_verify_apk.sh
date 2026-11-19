#!/usr/bin/env bash
# Verify an Android APK: signature (v2), versionName/versionCode, signer cert, and optionally the
# verified setup App Link.
# Usage: android_verify_apk.sh <apk> <versionName> <versionCode> [--expect-unsigned] [--app-link-host <host>]
#   --app-link-host: the APK's manifest must carry, on MainActivity, an autoVerify intent filter for
#   exactly https://<host>/setup (VIEW, DEFAULT, BROWSABLE; the build flag -PkaikuAppLinkHosts); a
#   release that lost it, or matches more than that exact path, fails (android_app_link_check.py).
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
  echo "$TREE" | python3 "$(dirname "$0")/android_app_link_check.py" "$APP_LINK_HOST" || fail "verified App Link filter for https://$APP_LINK_HOST/setup on MainActivity"
  echo "OK: verified App Link https://$APP_LINK_HOST/setup"
fi
