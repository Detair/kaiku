# Backup & Restore Runbook

> Companion to `infra/scripts/backup.sh`. Covers what is backed up, how to
> restore each piece, the periodic restore drill, and the one-time VPS
> verification checklist.
>
> Created 2026-06-12 (Goal 4, beta ops hardening). Deploys and all commands
> on the VPS are run by the operator — never automated.

## What is backed up

| Data | Mechanism | File pattern | Default retention |
|---|---|---|---|
| PostgreSQL (`voicechat` DB — all messages, guilds, users, keys metadata) | `pg_dump \| gzip` via the `canis-postgres` container | `kaiku-<ts>.sql.gz` | 7 days |
| RustFS object data (uploads, avatars, custom emoji) | tar of the docker volume | `kaiku-rustfs-<ts>.tar.gz` | 7 days |

Backups land in `/var/lib/kaiku/backups` on the VPS. Both archives are
integrity-checked (`gzip -t`) at creation time — an empty or truncated
archive fails the run loudly instead of rotting silently.

**Not covered:** Valkey (ephemeral: sessions/presence/rate-limit state —
acceptable loss), Caddy certs (re-issued via ACME), monitoring data
(Prometheus/Tempo/Loki — operational, not user data).

> ⚠️ Off-host copies: `/var/lib/kaiku/backups` lives on the same disk as the
> data it protects. Until off-host sync exists (e.g. rclone to external
> storage), a disk failure loses both. Tracked as a follow-up below.

## One-time VPS verification checklist (operator)

The repo cannot see the VPS; verify these once and tick them off:

- [ ] Backup cron installed: `crontab -l` shows
      `0 3 * * * /opt/kaiku/infra/scripts/backup.sh >> /var/log/kaiku-backup.log 2>&1`
- [ ] Recent backups exist and are non-trivial:
      `ls -lah /var/lib/kaiku/backups | tail` (a fresh `.sql.gz` from last
      night, plausible size)
- [ ] RustFS volume name matches the script default:
      `docker volume ls | grep rustfs` — if it isn't `compose_rustfs_data`,
      set `RUSTFS_VOLUME=<actual>` in the cron line
- [ ] Compose drift: `docker inspect canis-rustfs --format '{{.Config.Image}} {{.Mounts}}'`
      matches the `rustfs` service added to `infra/compose/docker-compose.yml`
      on 2026-06-12 (that service was reconstructed from the dev template
      because the live container predated its compose definition)

## Restore: PostgreSQL

```bash
# 1. Stop the app server so nothing writes mid-restore
docker stop canis-server

# 2. Recreate the database from the dump
gunzip -c /var/lib/kaiku/backups/kaiku-<ts>.sql.gz | \
  docker exec -i canis-postgres psql -U voicechat -d postgres \
    -c "DROP DATABASE IF EXISTS voicechat_restore;" \
    -c "CREATE DATABASE voicechat_restore OWNER voicechat;"
gunzip -c /var/lib/kaiku/backups/kaiku-<ts>.sql.gz | \
  docker exec -i canis-postgres psql -U voicechat -d voicechat_restore

# 3. Sanity-check the restored DB (counts should be plausible)
docker exec canis-postgres psql -U voicechat -d voicechat_restore \
  -c "SELECT count(*) FROM users;" -c "SELECT count(*) FROM messages;"

# 4. Swap: rename old DB aside, promote the restore
docker exec canis-postgres psql -U voicechat -d postgres -c \
  "ALTER DATABASE voicechat RENAME TO voicechat_old; \
   ALTER DATABASE voicechat_restore RENAME TO voicechat;"

# 5. Restart and verify health
docker start canis-server
curl -sf http://localhost:8080/health
```

Drop `voicechat_old` only after a day of verified operation.

## Restore: RustFS objects

```bash
docker stop canis-rustfs
docker run --rm \
  -v compose_rustfs_data:/data \
  -v /var/lib/kaiku/backups:/backup:ro \
  alpine sh -c "rm -rf /data/* && tar xzf /backup/kaiku-rustfs-<ts>.tar.gz -C /data"
docker start canis-rustfs
# Verify: open any older uploaded image in the app
```

## Restore drill (quarterly — ~20 minutes)

The backup you have never restored is a hope, not a backup.

1. Pick yesterday's `.sql.gz`; restore it into `voicechat_drill`
   (steps 2–3 above with the drill name — **skip the swap, step 4**).
2. Verify counts and spot-check one recent message:
   `SELECT content FROM messages ORDER BY created_at DESC LIMIT 1;`
3. Extract one file from the RustFS archive to /tmp and open it:
   `tar tzf kaiku-rustfs-<ts>.tar.gz | head` then extract one entry.
4. `DROP DATABASE voicechat_drill;` and delete the /tmp extraction.
5. Record the drill date below.

| Drill date | Backup used | Result | Operator |
|---|---|---|---|
| _none yet_ | | | |

## Follow-ups

- [ ] Off-host backup sync (rclone/restic target) — single-disk risk above
- [ ] Encrypt backups at rest (the spec promises "encrypted backups";
      today's archives are plaintext gzip on the VPS disk)
