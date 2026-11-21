# Kaiku Operations Runbooks

> Index for operators of self-hosted Kaiku instances. Every entry assumes
> the docker-compose deployment from `infra/compose/`.

## Runbooks

| Situation | Runbook |
|---|---|
| Taking / restoring backups, quarterly restore drill | [backup-restore-runbook.md](backup-restore-runbook.md) |
| Upgrading to a new release, rolling back a bad one | [upgrade-playbook.md](upgrade-playbook.md) |
| Cutting an RC or final release, one-time signing setup | [release-runbook.md](release-runbook.md) |
| Reading metrics/traces/logs, alert meanings | [observability-runbook.md](observability-runbook.md) |
| Observability data contract (what the server emits) | [observability-contract.md](observability-contract.md) |

## Quick triage

1. **One-call health snapshot** (system admin token required):
   `GET /api/admin/diagnostics` — store connectivity, DB pool pressure,
   disk usage, active voice/WS counts, error count (last 5 min).
2. **Liveness**: `GET /health` (no auth) — `ok` / `degraded` with per-store
   booleans.
3. **Metrics view**: admin dashboard → Observability, or Grafana
   (`--profile monitoring`).
4. **Before any deploy**: `infra/scripts/upgrade-preflight.sh` on the host.

## Scripts (run on the host)

| Script | Purpose |
|---|---|
| `infra/scripts/backup.sh` | Daily postgres + RustFS backup (cron) |
| `infra/scripts/upgrade-preflight.sh` | Pre-deploy safety gate |
| `infra/scripts/deploy.sh` | Pulls and restarts the images over ssh, but for one fixed host: not for general use; see the [upgrade playbook](upgrade-playbook.md) for the generic steps |
| `infra/scripts/setup-beta.sh` | Host provisioning for one fixed domain (set inside the script); builds the server image from the checkout. Not a general installer: see the [deployment guide](../admin-guide/ops/deployment.md) |

The compose stack runs the images `ghcr.io/detair/kaiku-server` and
`ghcr.io/detair/kaiku-web`, chosen with `KAIKU_VERSION` in
`infra/compose/.env`. Container names keep the `canis-` prefix (for example
`canis-server`, `canis-postgres`); the scripts and runbooks use them.
