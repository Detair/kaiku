# Upgrade Playbook

> Phase 8 "self-hosted upgrade safety" (Goal 5 item 3). How to upgrade a
> Kaiku deployment safely, and how to get back when an upgrade goes wrong.
> Companion script: `infra/scripts/upgrade-preflight.sh`.
> Backup/restore details: `docs/ops/backup-restore-runbook.md`.

## Compatibility matrix

Server and clients ship in **lockstep** — the WebSocket protocol and REST
surface are not versioned, so always upgrade server, web client and apps to
the same release.

| Server | Web client | Desktop client | Android | Status |
|---|---|---|---|---|
| release N | release N | release N | release N | ✅ the only supported combination |
| release N | release N | release N−1 | — | ⚠️ usually works for text chat; voice/WS event changes may break — not tested, don't rely on it |

The desktop app updates itself only between final releases. A release
candidate is updated by hand: install the newer version from the releases
page (the same applies to the Android APK).

This table must gain real rows when the protocol gets versioned
(prerequisite for non-lockstep upgrades and the desktop auto-updater
serving older servers).

## Safe upgrade windows

- Prefer low-activity hours (check Grafana for the daily usage trough, and
  pick a time after the nightly backup cron has produced a fresh dump).
- Never upgrade with active voice calls if avoidable: container restart
  drops all calls (clients auto-rejoin, but it's user-visible).
- Never upgrade with a failed or stale backup — the preflight enforces it.

## Upgrade procedure

All commands run on the host that runs the compose stack, from the checkout
(shown as `/opt/kaiku`, the path `infra/scripts/setup-beta.sh` uses; adjust it
and set `COMPOSE_DIR`/`MIGRATIONS_DIR`/`BACKUP_DIR` for the scripts if yours
differs). Deploys are operator-run, never automated.

```bash
# 0. Preflight — fix every ✗ before continuing
/opt/kaiku/infra/scripts/upgrade-preflight.sh
# Note the printed "Current server image" digest, and the running version:
docker inspect canis-server --format '{{.Config.Image}}'   # your rollback target

# 1. Fresh backup if the latest is more than a few hours old
/opt/kaiku/infra/scripts/backup.sh

# 2. Update the checkout (compose file and migrations of the new release)
cd /opt/kaiku && git fetch --tags && git checkout v<new-version>

# 3. Deploy the new release's images
cd infra/compose
#    set KAIKU_VERSION=<new-version> in .env (no leading "v")
docker compose pull server web
docker compose up -d                     # add --profile monitoring if you run it

# 4. Verify
curl -sf https://<your-domain>/health
docker logs canis-server --since 5m | grep -iE 'error|migrat' | head
# In the app: log in, open a channel, send a message, join voice briefly.
```

Migrations ship in the server image and apply automatically on server start.
Watch the logs in step 4 — a migration failure leaves the server
crash-looping and is your signal to roll back.

`infra/scripts/deploy.sh` is not a general deploy tool: it connects to one fixed
host. Use the steps above.

> The preflight's second check requests `http://localhost:8080/health`. The
> compose file does not publish the server's HTTP port on the host, so that
> check reports ✗ on a stock install even when the server is healthy; verify
> `/health` through your domain (step 4) and treat that single ✗ as known.

## Rollback

**Decide fast.** Two situations:

### A. New version misbehaves, no new migrations ran

Go back to the previous version (the one noted in step 0) and restart:

```bash
cd /opt/kaiku/infra/compose
# set KAIKU_VERSION=<previous-version> in .env
docker compose pull server web
docker compose up -d server web
curl -sf https://<your-domain>/health
```

Alternatively pin the digest printed by the preflight by editing the `server`
image line in `docker-compose.yml` to
`ghcr.io/detair/kaiku-server@sha256:<previous-digest>`, and revert that edit
after the next successful upgrade.

### B. New migrations ran (schema changed)

There are **no down-migrations** in this project (zero `.down.sql` files —
verified 2026-06-12). An applied migration cannot be reverted in place;
rolling back the binary alone may crash against the newer schema.

1. Go back to the previous version as in (A) — if the old server starts and works
   against the new schema (additive migration), you're done.
2. If it does not: restore the database from the pre-upgrade backup using
   the **Restore: PostgreSQL** procedure in
   `docs/ops/backup-restore-runbook.md`, then start the previous version.
   Messages sent between backup and rollback are lost — this is why the
   preflight refuses to run without a fresh backup.

## After any rollback

- Note what broke in the release's GitHub issue/PR before memories fade.
- Keep the failed image digest for debugging; don't retry the upgrade
  until the cause is understood.
