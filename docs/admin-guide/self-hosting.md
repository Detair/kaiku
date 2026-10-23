# <span style="color: #88C0D0;">Deploying Your Pack (Self-Hosting Kaiku)</span>

> [!WARNING]  
> **Early Development Note:** Kaiku is heavily in development. The architecture and deployment strategies described here represent our current structural roadmap and are actively being built. They are not yet ready for production deployment.

Kaiku was built from day one to reject corporate data harvesting. Self-hosting is not a second-class feature; it is the **primary** way Kaiku is meant to be experienced. 

This guide will (eventually) cover everything you need to know to deploy Kaiku for your community, whether you are a group of five friends or a massive esports organization.

---

## <span style="color: #88C0D0;">Core Philosophy</span>
If you bring the hardware, you own the data. 

Unlike platforms like Discord or TeamSpeak, a self-hosted Kaiku node grants you absolute sovereignty over:
- Voice metadata and analytics
- Direct message encryption keys
- Server roles and community metrics
- Uptime SLA and routing

## <span style="color: #88C0D0;">Infrastructure Overview</span>
A standard Kaiku deployment consists of three primary services:

1. **The Kaiku Client (Desktop/Web)**: The fast, Tauri-based application that your users actually interact with.
2. **The Signaling Server (Node)**: A lightweight WebSocket server responsible for matching peers and initiating WebRTC handshakes. 
3. **TURN/STUN Relays**: Necessary for bypassing strict NATs and firewalls so your users can always connect directly to each other.

### Recommended Specs (Initial Estimate)
Because the Heavy Lifting (Voice Encoding/Decoding) is handled client-side via WebRTC, the server footprint is incredibly small.
- **CPU**: 1-2 vCores
- **RAM**: 1GB (Signaling is incredibly efficient)
- **Bandwidth**: Dependent on TURN relay usage.

---

## <span style="color: #88C0D0;">Deployment Scenarios (Coming Soon)</span>

### 1. The "Lone Wolf" (Docker Compose)
The fastest way to get a node online. We will provide a pre-configured `docker-compose.yml` that spins up the Signaling Server, a coturn instance (for relaying), and a Redis cache in a single command. 

### 2. The "Esports Org" (Kubernetes)
For massive communities. Detailed Helm charts and documentation for deploying scalable replicas of the Signaling server behind a load balancer, with dedicated external TURN clusters.

## <span style="color: #88C0D0;">Voice NAT Traversal & Sovereignty (STUN/TURN)</span>

WebRTC needs a **STUN** server (to discover each client's public address) and,
for clients behind strict NATs/firewalls, a **TURN** relay. The bundled
`docker-compose.yml` runs a `coturn` instance that serves **both** on port
`3478`.

**Point STUN at your own coturn, not Google.** The server's default
`STUN_SERVER` is Google's public server (`stun:stun.l.google.com:19302`) so
voice works out of the box — but on a self-hosted node that still leaks every
client's IP-discovery request to a third party, defeating the point of
self-hosting. Set:

```env
STUN_SERVER=stun:<your-domain>:3478
TURN_SERVER=turn:<your-domain>:3478
TURN_SHARED_SECRET=<random 32+ byte hex>   # coturn use-auth-secret mode
```

`infra/scripts/setup-beta.sh` does this automatically (rewrites `STUN_SERVER`
to your domain and generates the shared secret). If you configure TURN but
leave STUN on Google, the server logs a startup **warning** flagging the leak.

TURN credentials are issued per-request as HMAC-SHA1 time-limited tokens
(RFC 5766), so no static password is stored client-side.

### Hardened defaults in the bundled compose file

The bundled `infra/compose/docker-compose.yml` ships with these settings:

- **Required variables.** `docker compose` refuses to start unless
  `VALKEY_PASSWORD` and `CORS_ALLOWED_ORIGINS` (e.g. `https://chat.example.com`)
  are set, next to the existing `POSTGRES_PASSWORD`, JWT keys, `DOMAIN` and
  `ACME_EMAIL`. Set `S3_ACCESS_KEY` / `S3_SECRET_KEY` when you use file
  uploads; they are passed to both RustFS and the server. The desktop app's
  origins are added by the server automatically.
- **Valkey password.** Use a hex password (`openssl rand -hex 24`). It is
  embedded in `REDIS_URL`, so characters such as `/`, `+`, `=` or `@`
  (typical for base64) break the URL. `setup-beta.sh` generates a hex password.
- **Trusted proxies.** The server trusts forwarded client addresses only from
  the compose network (`RATE_LIMIT_TRUSTED_PROXIES`, derived from
  `KAIKU_NET_SUBNET`).
- **`KAIKU_NET_SUBNET`** (default `172.31.250.0/24`): subnet of the compose
  application network. Change it if it collides with a LAN, VPN or other
  Docker network on the host.
- **`PUBLIC_IP`**: set it to the host's public address. The voice server
  advertises it in its ICE candidates, and the TURN server no longer relays to
  private addresses, so voice relies on it.
- **Voice UDP range** (`RTP_PORT_MIN`-`RTP_PORT_MAX`, default `10000`-`10100`):
  compose publishes this range and passes the same values to the server as
  `VOICE_UDP_PORT_MIN`/`VOICE_UDP_PORT_MAX`, so voice media binds only inside
  it. Forward exactly this UDP range 1:1 (same port numbers) on your router
  and firewall. Size it for at least 2 x concurrent voice participants x
  local addresses of the host, plus headroom; see
  [environment variables](configuration/environment.md).
- **Isolated TURN server.** `coturn` runs on its own `turn` network, cannot
  reach the application services, and refuses to relay to private, loopback,
  link-local and multicast addresses.

**Trade-off: no TURN/TCP relay.** coturn runs with `--no-tcp-relay`, so
relaying media over TCP is not available. Clients can still talk to the TURN
server over TCP (port 3478), but the relayed leg is UDP only. Clients on
networks that block all outgoing UDP cannot use voice through this TURN server.

### Upgrading existing deployments

When you update an existing compose installation:

1. **Add the required variables** to your `.env`: `VALKEY_PASSWORD` and
   `CORS_ALLOWED_ORIGINS`. Compose refuses to start without them.
2. **Use a hex Valkey password.** If your current password is base64 and
   contains `/`, `+` or `=`, generate a new one with `openssl rand -hex 24`,
   put it in `.env`, then recreate Valkey so it starts with the new password
   (`docker compose up -d --force-recreate valkey server`).
3. **Subnet change.** The application network now uses `KAIKU_NET_SUBNET`
   (default `172.31.250.0/24`). Docker cannot change the subnet of an existing
   network, so run `docker compose down` once (volumes are kept) and then
   `docker compose up -d` to recreate it. Pick another subnet if it overlaps
   with your own networks.
4. **Voice needs `PUBLIC_IP`.** Set it to the host's public address.
5. **No TURN over TCP relay.** Clients on networks that block all outgoing UDP
   can no longer relay voice through the bundled coturn.
6. **Rate-limit proxy setting.** Remove `RATE_LIMIT_TRUST_PROXY` from `.env`;
   it is replaced by `RATE_LIMIT_TRUSTED_PROXIES`, which the compose file now
   sets from `KAIKU_NET_SUBNET`.

---
*Documentation to be expanded as the implementation stabilizes.*
