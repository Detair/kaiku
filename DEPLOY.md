# VoiceChat Deployment Guide

This guide covers deploying VoiceChat on a self-hosted server.

## Prerequisites

- Linux server (Ubuntu 22.04+ recommended)
- Docker and Docker Compose v2
- Domain name pointing to your server
- Ports 80, 443 (TCP) and 10000-10100 (UDP) open

## Quick Start

```bash
# Clone the repository
git clone https://github.com/Wolftown-io/canis.git
cd canis

# Copy and configure environment
cp .env.example .env
nano .env  # Edit required settings

# Start services
cd infra/compose
docker compose up -d

# Check logs
docker compose logs -f server
```

## Configuration

### Required Settings

Edit `.env` with your values:

```bash
# Your domain (must have DNS pointing to this server)
DOMAIN=chat.yourdomain.com

# Generate secure passwords
POSTGRES_PASSWORD=$(openssl rand -base64 24)
JWT_SECRET=$(openssl rand -base64 32)

# Email for Let's Encrypt certificates
ACME_EMAIL=admin@yourdomain.com
```

### Voice Chat (WebRTC)

For voice to work, configure:

```bash
# Your server's public IP (required for NAT traversal)
PUBLIC_IP=203.0.113.50

# UDP port range (ensure firewall allows these)
RTP_PORT_MIN=10000
RTP_PORT_MAX=10100
```

**Firewall rules:**
```bash
# UFW
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 10000:10100/udp

# iptables
sudo iptables -A INPUT -p tcp --dport 80 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 443 -j ACCEPT
sudo iptables -A INPUT -p udp --dport 10000:10100 -j ACCEPT
```

### Optional: TURN Server

For users behind restrictive NATs, configure a TURN server:

```bash
TURN_SERVER=turn:turn.yourdomain.com:3478
TURN_USERNAME=voicechat
TURN_CREDENTIAL=your-turn-password
```

### Optional: S3 Storage

For file uploads, configure S3-compatible storage:

```bash
S3_ENDPOINT=https://s3.yourdomain.com
S3_BUCKET=voicechat
AWS_ACCESS_KEY_ID=your-access-key
AWS_SECRET_ACCESS_KEY=your-secret-key
```

### Optional: SSO/OIDC

For single sign-on with Authentik, Keycloak, etc:

```bash
OIDC_ISSUER_URL=https://auth.yourdomain.com/application/o/voicechat/
OIDC_CLIENT_ID=your-client-id
OIDC_CLIENT_SECRET=your-client-secret
```

## Services

| Service | Port | Description |
|---------|------|-------------|
| Traefik | 80, 443 | Reverse proxy with auto TLS |
| Server | 8080 (internal) | VoiceChat API + WebSocket |
| PostgreSQL | 5432 (internal) | Database |
| Redis | 6379 (internal) | Cache + pub/sub |

## Commands

```bash
# Start all services
docker compose up -d

# Stop all services
docker compose down

# View logs
docker compose logs -f [service]

# Rebuild after code changes
docker compose build --no-cache server
docker compose up -d server

# Database backup
docker compose exec postgres pg_dump -U voicechat voicechat > backup.sql

# Database restore
cat backup.sql | docker compose exec -T postgres psql -U voicechat voicechat
```

## Updating

```bash
cd canis
git pull origin main
cd infra/compose
docker compose build --no-cache server
docker compose up -d
```

## Troubleshooting

### Server won't start

```bash
# Check logs
docker compose logs server

# Common issues:
# - Database not ready: wait for postgres healthcheck
# - Missing env vars: check .env file
```

### Voice not working

1. Verify `PUBLIC_IP` is set correctly
2. Check UDP ports are open: `nc -vzu your-server 10000`
3. Check browser console for WebRTC errors
4. Consider adding a TURN server for restrictive NATs

### TLS certificate issues

```bash
# Check Traefik logs
docker compose logs traefik

# Verify DNS is pointing to server
dig +short chat.yourdomain.com

# Check Let's Encrypt rate limits if repeated failures
```

### Database issues

```bash
# Connect to database
docker compose exec postgres psql -U voicechat

# Check database size
docker compose exec postgres psql -U voicechat -c "SELECT pg_size_pretty(pg_database_size('voicechat'));"
```

## Architecture

```
                    ┌─────────────┐
                    │   Clients   │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
         TCP 80/443    TCP 443     UDP 10000-10100
              │        (WSS)        (RTP)
              │            │            │
        ┌─────┴─────┐      │            │
        │  Traefik  │      │            │
        └─────┬─────┘      │            │
              │            │            │
              └────────────┼────────────┘
                           │
                    ┌──────┴──────┐
                    │   Server    │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │                         │
        ┌─────┴─────┐            ┌──────┴──────┐
        │ PostgreSQL│            │    Redis    │
        └───────────┘            └─────────────┘
```

## Security Checklist

- [ ] Strong `POSTGRES_PASSWORD` (24+ random chars)
- [ ] Strong `JWT_SECRET` (32+ random chars)
- [ ] TLS enabled via Traefik
- [ ] Firewall configured (only 80, 443, UDP range open)
- [ ] Regular backups configured
- [ ] Log monitoring enabled

## Support

- Issues: https://github.com/Wolftown-io/canis/issues
- Discussions: https://github.com/Wolftown-io/canis/discussions
