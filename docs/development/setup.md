# Development Setup Guide

This guide covers setting up the VoiceChat development environment.

## Prerequisites

- Docker and Docker Compose
- Rust (latest stable)
- Bun (install: `curl -fsSL https://bun.sh/install | bash`)
- Node.js 18+ (required for Playwright tests)
- sqlx-cli: `cargo install sqlx-cli --no-default-features --features postgres`

## Quick Start

### 1. Start Development Services

Start PostgreSQL and Redis:

```bash
docker compose -f docker-compose.dev.yml up -d
```

This starts:
- **PostgreSQL** on port 5433 (user: `voicechat`, password: `voicechat_dev_pass`)
- **Redis** on port 6379

### 2. Configure Environment

Copy the example environment file and verify the configuration:

```bash
# If .env doesn't exist, copy from example
cp .env.example .env

# Verify the database password matches docker-compose.dev.yml
cat .env | grep DATABASE_URL
# Should show: postgres://voicechat:voicechat_dev_pass@localhost:5433/voicechat
```

**Important**: The development database password is `voicechat_dev_pass` (as configured in `docker-compose.dev.yml`).

### 3. Run Database Migrations

```bash
cd server
sqlx migrate run
```

### 4. Start the Server

```bash
cargo run -p vc-server
```

The server will start on http://localhost:8080

### 5. Start the Client (Tauri Desktop)

In a new terminal:

```bash
cd client
bun install
bun run tauri dev
```

This starts both the Vite dev server and the Tauri desktop app.

### 5a. Start the Web Frontend Only

For web-only development (no Tauri):

```bash
cd client
bun run dev
```

The web frontend will be available at http://localhost:5173

## Troubleshooting

### Database Connection Errors

If you see "password authentication failed for user 'voicechat'":

1. Check that Docker containers are running:
   ```bash
   docker compose -f docker-compose.dev.yml ps
   ```

2. Verify the password in `.env` matches the Docker container:
   ```bash
   # Check container environment
   docker exec voicechat-postgres-dev env | grep POSTGRES_PASSWORD
   # Should show: POSTGRES_PASSWORD=voicechat_dev_pass

   # Check .env file
   grep DATABASE_URL .env
   # Should include: voicechat_dev_pass
   ```

3. If the password is wrong, update `.env`:
   ```
   DATABASE_URL=postgres://voicechat:voicechat_dev_pass@localhost:5433/voicechat
   ```

### Port Already in Use

If you get "Address already in use" errors:

```bash
# Find and kill existing server process
pkill -f vc-server

# Or kill by port
lsof -ti:8080 | xargs kill
```

## Database Management

### Reset Database

```bash
# Stop all containers and remove volumes
docker compose -f docker-compose.dev.yml down -v

# Start fresh
docker compose -f docker-compose.dev.yml up -d

# Wait for PostgreSQL to be ready
sleep 3

# Run migrations (server does this automatically on startup)
cargo run -p vc-server
```

### Access Database

```bash
# Using docker exec
docker exec -it voicechat-postgres-dev psql -U voicechat -d voicechat

# Or using psql from host
PGPASSWORD=voicechat_dev_pass psql -h localhost -p 5433 -U voicechat -d voicechat
```

## Phase 3 Development Status

The following Phase 3 features are currently implemented:

✅ **Backend (Tasks 1-3):**
- Database migration with guild tables
- Complete guild REST API (8 endpoints)
- Channel guild scope integration

✅ **Frontend (Task 6):**
- Guild store with CRUD operations
- API integration (Tauri + browser HTTP fallback)
- Guild-scoped channel management

🚧 **In Progress:**
- Task 7: Server Rail UI
- Task 8: Context switching
- Tasks 4-5: Friends and DM backend
- Tasks 9-11: Friends UI, Home view, Rate limiting

## API Endpoints

### Guild Management
- `POST /api/guilds` - Create guild
- `GET /api/guilds` - List user's guilds
- `GET /api/guilds/:id` - Get guild details
- `PATCH /api/guilds/:id` - Update guild (owner only)
- `DELETE /api/guilds/:id` - Delete guild (owner only)
- `POST /api/guilds/:id/join` - Join guild with invite
- `POST /api/guilds/:id/leave` - Leave guild
- `GET /api/guilds/:id/members` - List guild members
- `GET /api/guilds/:id/channels` - List guild channels

All guild endpoints require authentication via JWT Bearer token.

## Testing

### Manual API Testing

```bash
# Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","password":"testpass123","email":"test@example.com"}'

# Login and get token
TOKEN=$(curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","password":"testpass123"}' | jq -r '.access_token')

# Create a guild
curl -X POST http://localhost:8080/api/guilds \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"My Server","description":"Test server"}'

# List guilds
curl http://localhost:8080/api/guilds \
  -H "Authorization: Bearer $TOKEN"
```

## Development Workflow

1. Make backend changes in `server/src/`
2. Database schema changes require new migration: `sqlx migrate add <name>`
3. Run migrations: `sqlx migrate run`
4. Test with `cargo test`
5. Make frontend changes in `client/src/`
6. Both server and client support hot reload during development

## Additional Resources

- [Project Specification](../project/specification.md)
- [Architecture Documentation](../architecture/overview.md)
- [Phase 3 Implementation Plan](docs/plans/PHASE_3_IMPLEMENTATION.md)
