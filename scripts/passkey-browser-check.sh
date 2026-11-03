#!/usr/bin/env bash
# Opt-in passkey check in a real Chromium (CDP virtual authenticator): web
# sign-in and registration, desktop ceremony page and loopback hand-off.
# Not part of CI. Starts vc-server on 127.0.0.1:8080 and the web dev server
# on https://localhost:5173 (the dev proxy's fixed ports), runs
# client/scripts/passkey-browser-check.mjs, stops both.
#
#   KAIKU_PASSKEY_BROWSER=1 \
#   DATABASE_URL=postgresql://…/passkey_browser REDIS_URL=redis://… \
#   ./scripts/passkey-browser-check.sh
#
# DATABASE_URL must be a throwaway database: the check creates accounts
# and passkeys and marks setup complete. SQL runs through `psql` unless
# KAIKU_PASSKEY_SQL names another command (SQL on stdin). JWT keys are
# generated for the run. See the .mjs header for the other variables.
set -euo pipefail

if [[ "${KAIKU_PASSKEY_BROWSER:-}" != "1" ]]; then
  echo "passkey browser check skipped (set KAIKU_PASSKEY_BROWSER=1)"
  exit 0
fi
: "${DATABASE_URL:?DATABASE_URL (throwaway database) is required}"
: "${REDIS_URL:?REDIS_URL is required}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

openssl genpkey -algorithm ed25519 -out "$WORK/jwt.pem"
openssl pkey -in "$WORK/jwt.pem" -pubout -out "$WORK/jwt.pub"

(cd "$ROOT" && SQLX_OFFLINE=true cargo build -q -p vc-server)
JWT_PRIVATE_KEY="$(base64 -w0 "$WORK/jwt.pem")" \
JWT_PUBLIC_KEY="$(base64 -w0 "$WORK/jwt.pub")" \
PUBLIC_URL=https://localhost:5173 \
BIND_ADDRESS=127.0.0.1:8080 \
RATE_LIMIT_ENABLED=false \
  "$ROOT/target/debug/vc-server" >"$WORK/server.log" 2>&1 &
PIDS+=($!)
(cd "$ROOT/client" && exec ./node_modules/.bin/vite --host localhost --port 5173 --strictPort) \
  >"$WORK/vite.log" 2>&1 &
PIDS+=($!)

for _ in $(seq 1 120); do
  if curl -sf http://127.0.0.1:8080/health >/dev/null && curl -skf https://localhost:5173/ >/dev/null; then
    break
  fi
  sleep 1
done
curl -sf http://127.0.0.1:8080/health >/dev/null || { tail -20 "$WORK/server.log"; exit 1; }

status=0
(cd "$ROOT/client" && KAIKU_PASSKEY_WEB_URL=https://localhost:5173 node scripts/passkey-browser-check.mjs) || status=$?
if [[ $status -ne 0 ]]; then
  echo "--- server log (last 30 lines)"
  tail -30 "$WORK/server.log"
fi
exit $status
