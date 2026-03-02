# Development Scripts

This directory contains utility scripts for development and maintenance.

## File Upload Setup

### `init-rustfs.sh`

Initializes RustFS (S3-compatible storage) for local development.

**Prerequisites**: `mc` ([MinIO Client](https://min.io/docs/minio/linux/reference/minio-mc.html)) installed locally, or use Docker method (recommended).

**Usage**:

```bash
# Method 1: Via Docker (recommended - no local installation needed)
docker run --rm --network container:canis-dev-rustfs --entrypoint sh minio/mc -c "\
  mc alias set local http://localhost:9000 rustfsdev rustfsdev_secret && \
  mc mb --ignore-existing local/voicechat && \
  mc anonymous set none local/voicechat"

# Method 2: Using the script (requires mc client on host)
./scripts/init-rustfs.sh
```

**What it does**:
1. Configures mc client to connect to local RustFS instance
2. Creates the `voicechat` bucket if it doesn't exist
3. Sets bucket policy to private (access via presigned URLs only)

**When to run**:
- First time setting up file uploads
- After running `docker compose down -v` (removes volumes)
- If file uploads return "Bucket not found" errors

See [File Uploads Development Guide](../docs/development/file-uploads.md) for complete setup instructions.

## Documentation Governance

### `check_docs_governance.py`

Validates documentation governance guardrails:

1. `docs/project/roadmap.md` metadata and plan links
2. `CHANGELOG.md` roadmap-alignment block consistency
3. `docs/plans/PLAN_LIFECYCLE.md` supersession integrity
4. `docs/project/RELEASE_NOTES_TEMPLATE.md` required headings

**Usage**:

```bash
python3 scripts/check_docs_governance.py
```

### `generate_release_notes.py`

Generates standardized milestone release notes from `CHANGELOG.md` `[Unreleased]`.

**Usage**:

```bash
python3 scripts/generate_release_notes.py \
  --version v0.1.0 \
  --output /tmp/release-notes.md
```

## Real Playwright Runner

### `run-e2e-real.sh`

Runs a real end-to-end Playwright flow from a clean state in one command:

1. Reset containers and volumes
2. Start PostgreSQL/Valkey/RustFS
3. Run SQL migrations
4. Initialize RustFS bucket
5. Start backend server
6. Run Playwright spec
7. Cleanup stack (unless `--keep-stack`)

**Usage**:

```bash
./scripts/run-e2e-real.sh
./scripts/run-e2e-real.sh --spec e2e/onboarding.spec.ts
./scripts/run-e2e-real.sh --spec e2e/gates.spec.ts --spec e2e/status-presence.spec.ts
./scripts/run-e2e-real.sh --project firefox -- --headed
```

By default, the runner executes Playwright with `--workers=1` for deterministic real-stack behavior.
Override with `PLAYWRIGHT_WORKERS=<n>` if needed.

You can also run it via Make:

```bash
make e2e-real
```

For a full real smoke batch (gates + status-presence + chat-core):

```bash
make e2e-real-smoke
```
