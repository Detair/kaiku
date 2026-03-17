# Caddy Migration — Replace Traefik + Stoat with Self-Contained Caddy

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the double-proxy setup (Stoat Caddy + Traefik) with a single Caddy service inside Kaiku's own docker-compose, making the deployment fully self-contained.

**Architecture:** Add Caddy as a service in `infra/compose/docker-compose.yml`. Caddy handles TLS (ACME), SPA serving from a named volume, API/auth/ws/health reverse proxy, and security headers. Remove Traefik entirely. Update deploy scripts and setup script.

**Tech Stack:** Caddy 2 (Alpine), Docker Compose

---

### Task 1: Create Caddyfile

**Files:**
- Create: `infra/compose/Caddyfile`

**Step 1: Write the Caddyfile**

```
{
	email {$ACME_EMAIL}
}

{$DOMAIN} {
	# API, auth, health, WebSocket — proxy to Rust backend
	handle /api/* {
		reverse_proxy server:8080
	}
	handle /auth/* {
		reverse_proxy server:8080
	}
	handle /health {
		reverse_proxy server:8080
	}
	handle /ws {
		reverse_proxy server:8080
	}

	# SPA — serve static files with fallback to index.html
	handle {
		root * /srv/kaiku
		try_files {path} /index.html
		file_server

		# Immutable hashed assets (Vite fingerprints these)
		@assets path /assets/*
		header @assets Cache-Control "public, max-age=31536000, immutable"

		# Security headers for the SPA
		header X-Frame-Options "DENY"
		header X-Content-Type-Options "nosniff"
		header Referrer-Policy "strict-origin-when-cross-origin"
		header Content-Security-Policy "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' wss://{$DOMAIN}; img-src 'self' blob: data:; media-src 'self' blob:; font-src 'self'; frame-ancestors 'none'"
	}
}
```

**Step 2: Commit**

```
feat(infra): add Caddyfile for SPA serving and API reverse proxy
```

---

### Task 2: Replace Traefik with Caddy in docker-compose.yml

**Files:**
- Modify: `infra/compose/docker-compose.yml`

**Step 1: Remove the Traefik service block** (lines 110-134)

Remove the entire `traefik:` service definition.

**Step 2: Remove Traefik labels from server service** (lines 60-67)

Remove all `labels:` with `traefik.*` from the server service.

**Step 3: Add `caddy` service**

Add after the valkey service block:

```yaml
  # ==========================================================================
  # Caddy — Reverse Proxy & SPA Server
  # ==========================================================================
  caddy:
    image: caddy:2-alpine
    container_name: canis-caddy
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
      - client_dist:/srv/kaiku:ro
    environment:
      - DOMAIN=${DOMAIN}
      - ACME_EMAIL=${ACME_EMAIL}
    networks:
      - voicechat
```

**Step 4: Update volumes**

Remove `letsencrypt:` volume. Add:

```yaml
  caddy_data:
  caddy_config:
  client_dist:
```

**Step 5: Add `depends_on: caddy` is NOT needed** — server and caddy are independent; caddy discovers server by DNS name on the `voicechat` network.

**Step 6: Build and verify compose config**

Run: `docker compose -f infra/compose/docker-compose.yml config --quiet` (syntax check)

**Step 7: Commit**

```
feat(infra): replace Traefik with Caddy in docker-compose

Removes Traefik service and labels. Adds Caddy with auto-TLS,
SPA serving from client_dist volume, and API reverse proxy.
```

---

### Task 3: Update deploy script

**Files:**
- Modify: `infra/scripts/deploy.sh`

**Step 1: Update `deploy_client()` function**

Change from copying to `stoat-caddy-1` to copying into the `client_dist` Docker volume via the `canis-caddy` container:

```bash
deploy_client() {
    echo "[+] Building and deploying client on VPS..."
    ssh "$VPS" 'bash -s' << 'REMOTE'
set -euo pipefail
export PATH="$HOME/.bun/bin:$PATH"
cd /opt/kaiku && git pull
cd client
echo "VITE_SERVER_URL=https://kaiku.pmind.de" > .env.production
bun install --frozen-lockfile 2>&1 | tail -1
bun run build 2>&1 | tail -2
docker cp dist/. canis-caddy:/srv/kaiku/
REMOTE
    echo "[+] Client deployed."
}
```

Key changes:
- `docker cp dist/. canis-caddy:/srv/kaiku/` (was `stoat-caddy-1`)
- No `docker restart` needed — Caddy serves files live

**Step 2: Commit**

```
feat(infra): update deploy script for Caddy (replaces Stoat)
```

---

### Task 4: Update setup script

**Files:**
- Modify: `infra/scripts/setup-beta.sh`

**Step 1: Update firewall comments**

Change lines 160-161 from:
```bash
ufw allow 80/tcp comment "HTTP (Traefik)"
ufw allow 443/tcp comment "HTTPS (Traefik)"
```
to:
```bash
ufw allow 80/tcp comment "HTTP (Caddy)"
ufw allow 443/tcp comment "HTTPS (Caddy)"
```

**Step 2: Update final output messages**

Change the "Useful commands" section to reference `caddy` instead of `traefik`:
```
docker compose logs -f caddy        # TLS/proxy logs
```

**Step 3: Commit**

```
chore(infra): update setup script references from Traefik to Caddy
```

---

### Task 5: Update CHANGELOG.md

**Files:**
- Modify: `CHANGELOG.md`

Add under `[Unreleased]` → `### Changed`:
```
- Replaced Traefik + external Stoat Caddy with self-contained Caddy reverse proxy in docker-compose, simplifying deployment to a single stack
- Added security headers (CSP, HSTS, X-Frame-Options) to SPA responses via Caddyfile
```

Add under `[Unreleased]` → `### Security`:
```
- Validation error responses no longer echo submitted field values (e.g., passwords)
- Added `Strict-Transport-Security` header to all API responses
- Non-guild-members now receive 403 instead of empty 200 on channel listing
- Display names are now rejected if they contain HTML tags
```

**Step: Commit**

```
docs: update CHANGELOG for security hardening and Caddy migration
```

---

## Out of Scope (VPS manual steps after deploy)

After merging and deploying, run on VPS:
1. `docker stop stoat-caddy-1 && docker rm stoat-caddy-1` (remove Stoat)
2. `cd /opt/kaiku/infra/compose && docker compose --profile monitoring up -d` (bring up new stack with Caddy)
3. Verify: `curl -I https://kaiku.pmind.de` shows Caddy headers
