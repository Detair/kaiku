#!/usr/bin/env bash
# Smoke test for the Kaiku web image (CI and release run this).
# usage: infra/docker/web/smoke-test.sh IMAGE [EXPECTED_VERSION]
# Uses $DOCKER (default: docker). Starts the image the way the docs recommend
# (read-only root filesystem, tmpfs /tmp, all capabilities dropped) and checks
# routing, caching and security headers.
set -euo pipefail

image="$1"
expected_version="${2:-}"
docker="${DOCKER:-docker}"
name="kaiku-web-smoke-$$"
port="${SMOKE_PORT:-18080}"

cleanup() { "$docker" rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; "$docker" logs "$name" >&2 || true; exit 1; }

# A bad media origin must stop the container instead of reaching the CSP.
if "$docker" run --rm -e KAIKU_MEDIA_ORIGINS="https://x.example.com; script-src *" "$image" >/dev/null 2>&1; then
    fail "container started with an invalid KAIKU_MEDIA_ORIGINS"
fi

"$docker" run -d --name "$name" -p "127.0.0.1:${port}:8080" \
    --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
    -e KAIKU_MEDIA_ORIGINS="https://files.example.com" "$image" >/dev/null

base="http://127.0.0.1:${port}"
for _ in $(seq 1 30); do
    curl -fsS "$base/healthz" >/dev/null 2>&1 && break
    sleep 1
done
curl -fsS "$base/healthz" | grep -qx ok || fail "/healthz"

headers() { curl -sS -o /dev/null -D - "$base$1" | tr -d '\r'; }
status() { curl -sS -o /dev/null -w '%{http_code}' "$base$1"; }
header() { headers "$1" | grep -i "^$2:" | head -1 | cut -d' ' -f2-; }

# App shell and client-side routes
curl -fsS "$base/" | grep -q '<div id="root">' || fail "/ does not serve the app"
curl -fsS "$base/login" | grep -q '<div id="root">' || fail "/login does not fall back to the app"
curl -fsS "$base/invite/abc123" | grep -q '<div id="root">' || fail "/invite/... does not fall back to the app"
[ "$(header / Cache-Control)" = "no-cache" ] || fail "index.html Cache-Control: $(header / Cache-Control)"

# Fingerprinted assets: long cache; a missing asset is a 404, not the app
asset="$(curl -fsS "$base/" | grep -oE '/assets/[^"]+\.js' | head -1)"
[ -n "$asset" ] || fail "no /assets/*.js referenced from index.html"
[ "$(status "$asset")" = 200 ] || fail "$asset not served"
header "$asset" Cache-Control | grep -q immutable || fail "$asset Cache-Control: $(header "$asset" Cache-Control)"
[ "$(status /assets/does-not-exist.js)" = 404 ] || fail "missing asset not 404"
[ "$(status /theme-preload.js)" = 200 ] || fail "/theme-preload.js not served"

# Server paths must not be answered with the app
for path in /api/settings /auth/login /ws /health; do
    [ "$(status "$path")" = 404 ] || fail "$path should be 404 in the web image"
done

# Security headers
csp="$(header / Content-Security-Policy)"
echo "CSP: $csp"
echo "$csp" | grep -q "script-src 'self';" || fail "CSP script-src"
echo "$csp" | grep -q "media-src 'self' blob: https://files.example.com;" || fail "CSP media-src without KAIKU_MEDIA_ORIGINS"
echo "$csp" | grep -q "img-src 'self' data: blob: https: https://files.example.com;" || fail "CSP img-src without KAIKU_MEDIA_ORIGINS"
echo "$csp" | grep -q "connect-src 'self' wss://127.0.0.1:${port} https://files.example.com;" \
    || echo "$csp" | grep -q "connect-src 'self' wss://127.0.0.1 https://files.example.com;" \
    || fail "CSP connect-src"
echo "$csp" | grep -q "frame-ancestors 'none'" || fail "CSP frame-ancestors"
[ "$(header / X-Content-Type-Options)" = "nosniff" ] || fail "X-Content-Type-Options"
[ "$(header / X-Frame-Options)" = "DENY" ] || fail "X-Frame-Options"
header / Server | grep -qv '[0-9]' || fail "Server header leaks the nginx version"

# Runs as the unprivileged user
uid="$("$docker" exec "$name" id -u)"
[ "$uid" = 101 ] || fail "runs as uid $uid, expected 101"

if [ -n "$expected_version" ]; then
    label="$("$docker" image inspect "$image" --format '{{ index .Config.Labels "org.opencontainers.image.version" }}')"
    [ "$label" = "$expected_version" ] || fail "version label '$label' != '$expected_version'"
fi

echo "web image smoke test passed"
