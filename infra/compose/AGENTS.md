# Docker Compose

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Docker Compose configurations for running development and testing infrastructure services. Provides containerized PostgreSQL, Redis, RustFS (S3), and MailHog for local development.

## Key Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | Complete development environment definition |

## For AI Agents

### Services

The compose file defines 4 services:

| Service | Image | Purpose | Ports |
|---------|-------|---------|-------|
| `postgres` | postgres:15-alpine | Primary database | 5432 |
| `redis` | redis:7-alpine | Cache, sessions, presence | 6379 |
| `rustfs` | rustfs/rustfs:latest | S3-compatible object storage | 9000 (API), 9001 (Console) |
| `mailhog` | mailhog/mailhog:latest | Email testing (dev only) | 1025 (SMTP), 8025 (Web UI) |

### PostgreSQL configuration

**Image:** `postgres:15-alpine`
**Purpose:** Primary relational database

**Environment:**
- `POSTGRES_USER=voicechat` — Default superuser
- `POSTGRES_PASSWORD=devpassword` — **DEV ONLY** password
- `POSTGRES_DB=voicechat` — Default database

**Volumes:**
- `postgres_data:/var/lib/postgresql/data` — Persistent data
- `../docker/init-scripts:/docker-entrypoint-initdb.d` — Initialization scripts

**Healthcheck:** `pg_isready` every 5s

**Init scripts:**
Scripts in `/docker-entrypoint-initdb.d/` run on first startup:
- `01-grant-test-permissions.sql` — Grant necessary test permissions

### Redis configuration

**Image:** `redis:7-alpine`
**Purpose:** Caching, sessions, rate limiting, presence tracking

**Command:** `redis-server --appendonly yes`
- Enables AOF persistence for durability

**Volumes:**
- `redis_data:/data` — Persistent data

**Healthcheck:** `redis-cli ping` every 5s

### RustFS configuration

**Image:** `rustfs/rustfs:latest`
**Purpose:** S3-compatible object storage for file uploads (avatars, attachments)

**Command:** `--console-enable /data`

**Environment:**
- `RUSTFS_ACCESS_KEY=rustfsdev` — Admin access key
- `RUSTFS_SECRET_KEY=rustfsdev_secret` — **DEV ONLY** secret key

**Ports:**
- `9000` — S3 API endpoint
- `9001` — Web console

**Volumes:**
- `rustfs_dev_data:/data` — Persistent storage

**Healthcheck:** `curl -f http://localhost:9000/health` every 30s

**Web console:** http://localhost:9001/rustfs/console/index.html

### MailHog configuration

**Image:** `mailhog/mailhog:latest`
**Purpose:** Capture outgoing emails during development (registration, password reset)

**Ports:**
- `1025` — SMTP server (app sends here)
- `8025` — Web UI to view captured emails

**NO persistence** (emails cleared on restart)

**Web UI:** http://localhost:8025

### Usage

**Start all services:**
```bash
cd infra/compose
docker compose up -d
```

**View logs:**
```bash
docker compose logs -f [service_name]
```

**Stop services:**
```bash
docker compose down
```

**Stop and remove volumes (clean slate):**
```bash
docker compose down -v
```

**Check service health:**
```bash
docker compose ps
```

### Connection strings

Services are accessible on localhost:

**PostgreSQL:**
```
DATABASE_URL=postgresql://voicechat:devpassword@localhost:5432/voicechat
```

**Redis:**
```
REDIS_URL=redis://localhost:6379
```

**RustFS (S3):**
```
S3_ENDPOINT=http://localhost:9000
AWS_ACCESS_KEY_ID=rustfsdev
AWS_SECRET_ACCESS_KEY=rustfsdev_secret
S3_BUCKET=voicechat
```

**SMTP (MailHog):**
```
SMTP_HOST=localhost
SMTP_PORT=1025
SMTP_USER=
SMTP_PASSWORD=
```

### Modifying services

**Add new service:**
```yaml
services:
  new_service:
    image: service:latest
    ports:
      - "port:port"
    environment:
      - ENV_VAR=value
    volumes:
      - volume_name:/path
    healthcheck:
      test: ["CMD", "healthcheck-command"]
      interval: 10s
      timeout: 5s
      retries: 3

volumes:
  volume_name:
```

**Update service version:**
1. Change `image:` tag
2. Run `docker compose pull`
3. Run `docker compose up -d` (recreates containers)

### Environment overrides

To customize configuration without modifying the file:

**Create `.env` in compose directory:**
```env
POSTGRES_PASSWORD=custom_password
REDIS_PORT=6380
```

**Reference in docker-compose.yml:**
```yaml
environment:
  - POSTGRES_PASSWORD=${POSTGRES_PASSWORD:-devpassword}
ports:
  - "${REDIS_PORT:-6379}:6379"
```

### Troubleshooting

**Port already in use:**
```bash
# Find what's using the port
sudo lsof -i :5432
# Change port in docker-compose.yml
ports:
  - "5433:5432"  # Host:Container
```

**Container won't start:**
```bash
docker compose logs service_name
docker compose ps  # Check health status
```

**Reset PostgreSQL:**
```bash
docker compose down
docker volume rm compose_postgres_data
docker compose up -d postgres
```

**Reset all data:**
```bash
docker compose down -v  # Removes all volumes
docker compose up -d
```

### Production differences

**DO NOT use this in production:**
- Passwords are hardcoded (use secrets)
- No TLS/SSL (use encrypted connections)
- No resource limits (use `deploy.resources`)
- No backup strategy (implement automated backups)
- MailHog is dev-only (use real SMTP in prod)

**For production:**
- Use managed services (RDS, ElastiCache, S3)
- OR use hardened container configs with secrets, TLS, backups
- See `../../DEPLOY.md` for production deployment guide

### Performance tuning

**For development:**
Current config is optimized for development (low resource usage).

**For load testing:**
Increase resource limits:
```yaml
services:
  postgres:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
    command:
      - postgres
      - -c
      - max_connections=200
      - -c
      - shared_buffers=512MB
```

### Healthchecks

All services include healthchecks:
- Postgres: `pg_isready -U voicechat`
- Redis: `redis-cli ping`
- RustFS: `curl -f http://localhost:9000/health`
- MailHog: None (optional service)

Docker Compose waits for healthy status before marking services as ready.

### Dependencies

Services can depend on others:
```yaml
services:
  app:
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
```

Currently not used (app handles retries), but can be added if needed.
