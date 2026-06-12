# Upgrade Playbook

> Phase 8 "self-hosted upgrade safety" (Goal 5 item 3). How to upgrade a
> Kaiku deployment safely, and how to get back when an upgrade goes wrong.
> Companion script: `infra/scripts/upgrade-preflight.sh`.
> Backup/restore details: `docs/ops/backup-restore-runbook.md`.

## Compatibility matrix (beta)

During the 0.1.x beta, server and clients ship in **lockstep** — the
WebSocket protocol and REST surface are not yet versioned, so always
upgrade server and clients from the same release tag.

| Server | Desktop client | Android | Status |
|---|---|---|---|
| 0.1.x (tag N) | 0.1.x (tag N) | 0.1.x | ✅ the only supported combination |
| tag N | tag N−1 | — | ⚠️ usually works for text chat; voice/WS event changes may break — not tested, don't rely on it |

This table must gain real rows when the protocol gets versioned
(prerequisite for non-lockstep upgrades and the desktop auto-updater
serving older servers).

## Safe upgrade windows

- Prefer low-activity hours (check Grafana for the daily usage trough;
  for the beta server that is typically **03:00–07:00 UTC**, after the
  03:00 backup cron has produced a fresh dump).
- Never upgrade with active voice calls if avoidable: container restart
  drops all calls (clients auto-rejoin, but it's user-visible).
- Never upgrade with a failed or stale backup — the preflight enforces it.

## Upgrade procedure

All commands run on the VPS unless noted. Deploys are operator-run, never
automated.

```bash
# 0. Preflight — fix every ✗ before continuing
/opt/kaiku/infra/scripts/upgrade-preflight.sh
# Note the printed "Current server image" digest — that is your rollback pin.

# 1. Fresh backup if the latest is more than a few hours old
/opt/kaiku/infra/scripts/backup.sh

# 2. Update the checkout (compose files, migrations, client source)
cd /opt/kaiku && git pull

# 3. Deploy (from the workstation, or run the equivalent on the VPS)
./infra/scripts/deploy.sh              # server + client
#   --server-only / --client-only as needed

# 4. Verify
curl -sf http://localhost:8080/health
docker logs canis-server --since 5m | grep -iE 'error|migrat' | head
# In the app: log in, open a channel, send a message, join voice briefly.
```

Migrations apply automatically on server start. Watch the logs in step 4 —
a migration failure leaves the server crash-looping and is your signal to
roll back.

## Rollback

**Decide fast.** Two situations:

### A. New version misbehaves, no new migrations ran

Pin the previous image (digest printed by the preflight) and restart:

```bash
cd /opt/kaiku/infra/compose
# Edit docker-compose.yml: replace the server image line with the digest, e.g.
#   image: ghcr.io/detair/kaiku/server@sha256:<previous-digest>
docker compose --profile monitoring up -d server
curl -sf http://localhost:8080/health
```

Revert the compose edit after the next successful upgrade.

### B. New migrations ran (schema changed)

There are **no down-migrations** in this project (zero `.down.sql` files —
verified 2026-06-12). An applied migration cannot be reverted in place;
rolling back the binary alone may crash against the newer schema.

1. Pin the previous image as in (A) — if the old server starts and works
   against the new schema (additive migration), you're done.
2. If it does not: restore the database from the pre-upgrade backup using
   the **Restore: PostgreSQL** procedure in
   `docs/ops/backup-restore-runbook.md`, then start the pinned image.
   Messages sent between backup and rollback are lost — this is why the
   preflight refuses to run without a fresh backup.

## After any rollback

- Note what broke in the release's GitHub issue/PR before memories fade.
- Keep the failed image digest for debugging; don't retry the upgrade
  until the cause is understood.
