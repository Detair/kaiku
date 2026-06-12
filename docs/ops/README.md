# Kaiku Operations Runbooks

> Index for operators of self-hosted Kaiku instances. Every entry assumes
> the docker-compose deployment from `infra/compose/`.

## Runbooks

| Situation | Runbook |
|---|---|
| Taking / restoring backups, quarterly restore drill | [backup-restore-runbook.md](backup-restore-runbook.md) |
| Upgrading to a new release, rolling back a bad one | [upgrade-playbook.md](upgrade-playbook.md) |
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
| `infra/scripts/deploy.sh` | Operator-run deploy (never automated) |
| `infra/scripts/setup-beta.sh` | Initial host provisioning |
