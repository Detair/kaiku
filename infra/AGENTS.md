<!-- Parent: ../AGENTS.md -->
# Infrastructure

## Purpose
Docker configurations and deployment scripts for development and production environments.

## Subdirectories
- `compose/` - Docker Compose files
- `docker/` - Dockerfiles and related configs

## For AI Agents

### Development Services
The development environment uses Docker Compose to run:
- **PostgreSQL 15** - Primary database
- **Redis 7** - Sessions, caching, presence
- **RustFS** - S3-compatible file storage
- **MailHog** - Email testing (development only)

### Quick Start
```bash
# Start all development services
make docker-up
# Or directly
cd infra/compose && docker compose up -d

# View logs
make docker-logs
# Or
docker compose -f infra/compose/docker-compose.yml logs -f

# Stop services
make docker-down
```

### Environment Variables
Development services use defaults from `.env.example`. For production, customize:
- `DATABASE_URL` - PostgreSQL connection string
- `REDIS_URL` - Redis connection string
- `S3_*` - RustFS/S3 configuration

### Production Deployment
See `../DEPLOY.md` for production deployment instructions.

## Dependencies
- Docker & Docker Compose
- PostgreSQL 15+
- Redis 7+
- RustFS (S3-compatible)
