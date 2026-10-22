# Docker

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Dockerfiles and related configurations for building production container images and database initialization scripts.

## Key Files

| File | Purpose |
|------|---------|
| `Dockerfile` | Multi-stage build for Kaiku server |
| `init-scripts/01-grant-test-permissions.sh` | PostgreSQL initialization script |

## Subdirectories

### `init-scripts/`
Database initialization scripts mounted into PostgreSQL container. Run automatically on first startup.

## For AI Agents

### Dockerfile

**Purpose:** Build production-ready Kaiku server image

**Build strategy:** Multi-stage build
1. **builder** — Compile Rust binary with optimizations
2. **runtime** — Minimal runtime image with only binary and dependencies

**Current structure:** see `Dockerfile` (builder `rust:1.95-bookworm`, runtime `bitnami/minideb:bookworm`, runs as UID 1001). The runtime image contains exactly `/app/vc-server` and `/app/migrations`; configuration is environment variables only (see `docs/admin-guide/configuration/environment.md`).

### Published image

- **Image:** `ghcr.io/detair/kaiku-server` (amd64).
- **Release tags:** `<version>` without leading `v` (release candidates included). `latest` moves only for final releases, never for `-rc.N`.
- **`main` builds** (`ci.yml`) push `main` and `sha-<short>` and never `latest`.
- **Publish rule:** only the Release workflow publishes versioned tags, and only on a tag push or a dispatch with `dry_run` unticked. Dry runs and PRs build and smoke-test but do not push.
- `release.yml` smoke-tests the built image before pushing (binary runs, `/app` contents, version label).

### Building the image

**Local build:**
```bash
docker build -f infra/docker/Dockerfile -t kaiku-server:latest .
```

**Multi-platform build:**
```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f infra/docker/Dockerfile \
  -t kaiku-server:latest \
  .
```

### Runtime image

**Base:** `bitnami/minideb:bookworm` (minimal Debian)
- Includes libc and basic utilities
- Missing: Postgres client, development tools

**Required runtime dependencies:**
- `libssl3` — TLS support (OpenSSL)
- `ca-certificates` — For HTTPS connections
- `libpq5` — PostgreSQL client library (if using sqlx)

**Add to runtime stage if needed:**
```dockerfile
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*
```

### Optimization strategies

**Current size:** TBD (depends on final binary size)

**Size optimizations:**
```dockerfile
# Use Alpine (smaller base image, but musl libc compatibility issues)
FROM alpine:3.19 AS runtime

# Strip debug symbols from binary
RUN strip /usr/local/bin/vc-server
```

**Build speed optimizations:**
- Use cargo-chef for layer caching
- Cache `/usr/local/cargo/registry` and `target/` in CI
- Use `sccache` for distributed build caching

### Security hardening

**Run as non-root user:**
```dockerfile
FROM debian:bookworm-slim
RUN useradd -m -u 1000 appuser
USER appuser
COPY --from=builder --chown=appuser:appuser /app/target/release/vc-server /usr/local/bin/
ENTRYPOINT ["vc-server"]
```

**Read-only root filesystem:**
```dockerfile
ENTRYPOINT ["vc-server"]
# In docker-compose.yml or k8s:
# security_opt:
#   - no-new-privileges:true
# read_only: true
# tmpfs:
#   - /tmp
```

**Minimal attack surface:**
- Don't install unnecessary packages
- Use specific package versions
- Regularly update base image

### Environment configuration

**Required environment variables:**
```env
DATABASE_URL=postgresql://user:pass@host:5432/db
JWT_PRIVATE_KEY=<base64 PEM>
JWT_PUBLIC_KEY=<base64 PEM>
# Optional: REDIS_URL, BIND_ADDRESS, ... (full list in environment.md)
```

**Pass via docker run:**
```bash
docker run -e DATABASE_URL=... -e REDIS_URL=... kaiku-server:latest
```

**Pass via .env file:**
```bash
docker run --env-file .env kaiku-server:latest
```

### Health checks

**Add to Dockerfile:**
```dockerfile
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1
```

**Requires curl in runtime image:**
```dockerfile
RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*
```

### init-scripts/

**Purpose:** Initialize PostgreSQL database on first startup

**How it works:**
1. Scripts are mounted to `/docker-entrypoint-initdb.d/` in postgres container
2. PostgreSQL runs scripts in alphabetical order on first startup
3. Only runs if data directory is empty

**01-grant-test-permissions.sh:**
Grants necessary permissions for running tests:
```bash
#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    -- Grant permissions for test user
    GRANT CREATE ON DATABASE "$POSTGRES_DB" TO "$POSTGRES_USER";
EOSQL
```

**Adding new init script:**
1. Create `XX-description.sh` (XX = numeric prefix for ordering)
2. Make executable: `chmod +x infra/docker/init-scripts/XX-description.sh`
3. Use proper error handling: `set -e` at top
4. Run SQL via `psql` command

**Example init script:**
```bash
#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
    CREATE EXTENSION IF NOT EXISTS "pg_trgm";

    -- Create additional database for testing
    CREATE DATABASE voicechat_test;
    GRANT ALL PRIVILEGES ON DATABASE voicechat_test TO $POSTGRES_USER;
EOSQL
```

### Testing the Dockerfile

**Build and run locally:**
```bash
# Build
docker build -f infra/docker/Dockerfile -t kaiku-server:test .

# Run with compose services
cd infra/compose
docker compose up -d

# Run server container
docker run --rm \
  --network compose_default \
  -e DATABASE_URL=postgresql://voicechat:devpassword@postgres:5432/voicechat \
  -e REDIS_URL=redis://redis:6379 \
  -e JWT_PRIVATE_KEY=... -e JWT_PUBLIC_KEY=... \
  -p 8080:8080 \
  kaiku-server:test
```

### CI/CD integration

**Build in GitHub Actions:**
```yaml
- name: Build Docker image
  run: docker build -f infra/docker/Dockerfile -t kaiku-server:${{ github.sha }} .

- name: Push to registry
  run: |
    docker tag kaiku-server:${{ github.sha }} ghcr.io/detair/kaiku-server:<version>
    docker push ghcr.io/detair/kaiku-server:<version>
```

**Use buildx for multi-platform:**
```yaml
- name: Set up Docker Buildx
  uses: docker/setup-buildx-action@v3

- name: Build and push
  uses: docker/build-push-action@v5
  with:
    context: .
    file: infra/docker/Dockerfile
    platforms: linux/amd64,linux/arm64
    push: true
    tags: ghcr.io/detair/kaiku-server:<version>
```

### Troubleshooting

**Build fails:**
```bash
# Check build logs
docker build --progress=plain -f infra/docker/Dockerfile .

# Build specific stage
docker build --target builder -f infra/docker/Dockerfile .
```

**Runtime errors:**
```bash
# Check container logs
docker logs container_id

# Exec into running container
docker exec -it container_id /bin/bash

# Inspect container
docker inspect container_id
```

**Init scripts not running:**
- Ensure scripts are executable: `chmod +x infra/docker/init-scripts/*.sh`
- Check script has `#!/bin/bash` shebang
- Verify `set -e` for error propagation
- Check postgres logs: `docker compose logs postgres`

### Production deployment

See `../../DEPLOY.md` for production deployment guide.

**Container orchestration options:**
- Docker Compose (simple deployments)
- Kubernetes (scalable deployments)
- Docker Swarm (alternative to k8s)
- Managed container services (ECS, Cloud Run, etc.)

### Maintenance

**Update base images regularly:**
```bash
# Pull latest base
docker pull rust:1.95-bookworm
docker pull bitnami/minideb:bookworm

# Rebuild
docker build -f infra/docker/Dockerfile -t kaiku-server:latest .
```

**Scan for vulnerabilities:**
```bash
docker scan kaiku-server:latest
# Or use trivy
trivy image kaiku-server:latest
```
