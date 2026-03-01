<!-- Parent: ../AGENTS.md -->
# Server

## Purpose
Backend server for the Kaiku platform. Built with Rust and Axum, providing REST API, WebSocket connections, and voice service coordination.

## Key Files
- `Cargo.toml` - Server crate configuration
- `setup-db.sh` - Database setup script
- `SECURITY.md` - Security considerations and threat model
- `.env` - Local environment configuration

## Subdirectories
- `src/` - Server source code - see src/AGENTS.md
- `migrations/` - SQLx database migrations
- `seeds/` - Test data for development
- `tests/` - Integration tests

## For AI Agents

### Architecture
The server follows a service-oriented architecture with clear boundaries:
- **API Layer** (`src/api/`) - HTTP routes and handlers
- **WebSocket Layer** (`src/ws/`) - Real-time message routing
- **Service Modules** - Business logic (auth, chat, voice, etc.)
- **Data Layer** (`src/db/`) - PostgreSQL via SQLx

### Critical Security Paths
- `src/auth/` - Authentication (JWT, OIDC, MFA) - requires security review
- `src/permissions/` - Authorization checks - all changes need review
- `src/ratelimit/` - Rate limiting - protect against abuse

### Performance-Critical Paths
- `src/voice/` - Voice service (SFU coordination) - <50ms latency target
- `src/ws/` - WebSocket message routing - must be fast

### Running the Server
```bash
# Development mode with auto-reload
cargo watch -x 'run -p vc-server'
# Or via Makefile
make dev

# Run tests
cargo test -p vc-server
```

### Database Operations
```bash
# Run migrations
sqlx migrate run --source server/migrations

# Create new migration
sqlx migrate add -r <name> --source server/migrations

# Revert last migration
sqlx migrate revert --source server/migrations
```

## Dependencies
- Axum (web framework)
- SQLx (PostgreSQL async client)
- fred (Redis client)
- jsonwebtoken (JWT handling)
- vodozemac (E2EE text chat)
- webrtc-rs (via vc-common types)
