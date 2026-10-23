# Server Environment Variables

The Kaiku server image (`ghcr.io/detair/kaiku-server:<version>`) is configured with
environment variables only.

## Image contract

- **Env vars only.** No configuration files are baked into the image; the binary reads
  everything from its environment at startup. Without `DATABASE_URL` it exits with
  `DATABASE_URL must be set`.
- **Migrations ship in the image** at `/app/migrations`. `/app` contains exactly
  `vc-server` and `migrations`.
- **Tags.** `<version>` (no leading `v`) for every release, including release candidates.
  `latest` only moves for final (non-pre-release) versions. Builds of `main` are tagged
  `main` and `sha-<short>` and never `latest`.
- Platform: `linux/amd64`. Runs as UID 1001. HTTP on `8080`.

"Required" means the server refuses to start without the variable. Boolean variables accept
`true` or `1` (case-insensitive); anything else is false. An empty value counts as unset for
variables marked "empty = unset".

## Server

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `BIND_ADDRESS` | `0.0.0.0:8080` | Address and port the HTTP server listens on |
| `KAIKU_ENV` | `production` | Deployment environment name |
| `PUBLIC_BASE_URL` | unset (empty = unset) | Public base URL for absolute URLs handed to external services (webhook execute URLs); falls back to the request Host header |
| `PUBLIC_IP` | unset (empty = unset) | Public IP advertised for voice (WebRTC) connections (NAT 1:1 mapping). Required when the server runs behind NAT or in a container so clients can reach the SFU. The advertised candidate port is the bound port (no port translation), so the firewall/NAT must forward the identical UDP port numbers 1:1, including the `VOICE_UDP_PORT_MIN`-`VOICE_UDP_PORT_MAX` range. A `PUBLIC_IP` of the family excluded by `VOICE_ICE_IP_FAMILY` is ignored (a warning is logged) |
| `CORS_ALLOWED_ORIGINS` | `*` | Comma-separated list of allowed CORS origins. The desktop app origins `tauri://localhost`, `http://tauri.localhost` and `https://tauri.localhost` are added automatically to an explicit list |
| `ENABLE_API_DOCS` | `false` in release builds | Serve the OpenAPI docs |
| `ENABLE_GUILD_DISCOVERY` | `true` | Enable public guild discovery |
| `GEOIP_API_URL` | `http://ip-api.com/json/{ip}?fields=city,country` | GeoIP lookup endpoint for session locations; `{ip}` is replaced |
| `MAX_GUILDS_PER_USER` | `100` | Guilds a user can join (minimum 1) |
| `MAX_MEMBERS_PER_GUILD` | `1000` | Members per guild (minimum 1) |
| `MAX_CHANNELS_PER_GUILD` | `200` | Channels per guild (minimum 1) |
| `MAX_ROLES_PER_GUILD` | `50` | Roles per guild (minimum 1) |
| `MAX_EMOJIS_PER_GUILD` | `50` | Custom emojis per guild (minimum 1) |
| `MAX_BOTS_PER_GUILD` | `10` | Bots per guild (minimum 1) |
| `MAX_WEBHOOKS_PER_APP` | `5` | Webhooks per bot application (minimum 1) |
| `MAX_WORKSPACES_PER_USER` | `20` | Workspaces per user (minimum 1) |
| `MAX_ENTRIES_PER_WORKSPACE` | `50` | Entries per workspace (minimum 1) |
| `MAX_PAGES_PER_GUILD` | `10` | Pages per guild (minimum 1) |
| `MAX_REVISIONS_PER_PAGE` | `25` | Revisions kept per page (minimum 1) |

## Database

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `DATABASE_URL` | **required** | PostgreSQL connection string |

## Valkey

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `REDIS_URL` | `redis://localhost:6379` | Valkey (Redis-compatible) connection URL |

Rate limiting also uses Valkey; see [rate-limiting.md](rate-limiting.md).

## Authentication

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `JWT_PRIVATE_KEY` | **required** | Base64-encoded PEM private key that signs access tokens |
| `JWT_PUBLIC_KEY` | **required** | Base64-encoded PEM public key that verifies access tokens |
| `JWT_ACCESS_EXPIRY` | `900` | Access token lifetime in seconds |
| `JWT_REFRESH_EXPIRY` | `604800` | Refresh token lifetime in seconds (7 days) |
| `MFA_ENCRYPTION_KEY` | unset (empty = unset) | 32-byte hex key that encrypts MFA secrets at rest |
| `REQUIRE_E2EE_SETUP` | `false` | Require users to set up end-to-end encryption |
| `BLOCK_CHECK_FAIL_OPEN` | `false` | Allow DMs/calls when the Valkey block check fails (default fail-closed) |
| `COOKIE_SECURE` | `true` in release builds | `Secure` flag on the refresh cookie |
| `COOKIE_DOMAIN` | unset | Domain of the refresh cookie |
| `COOKIE_SAMESITE` | `lax` | `lax`, `strict` or `none`; `none` requires `COOKIE_SECURE=true` |
| `OIDC_ISSUER_URL` | unset (empty = unset) | OIDC provider issuer URL (seeds an OIDC provider) |
| `OIDC_CLIENT_ID` | unset (empty = unset) | OIDC client ID |
| `OIDC_CLIENT_SECRET` | unset (empty = unset) | OIDC client secret |
| `SMTP_HOST` | unset (empty = unset) | SMTP server for outgoing mail |
| `SMTP_PORT` | `587` | SMTP port |
| `SMTP_USERNAME` | unset (empty = unset) | SMTP username |
| `SMTP_PASSWORD` | unset (empty = unset) | SMTP password |
| `SMTP_FROM` | unset (empty = unset) | Sender address of outgoing mail |
| `SMTP_TLS` | `starttls` | SMTP TLS mode |

## Storage

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `S3_ENDPOINT` | unset (empty = unset) | S3-compatible endpoint (for example MinIO) |
| `S3_BUCKET` | `voicechat` | Bucket for uploads |
| `S3_PUBLIC_URL` | unset (empty = unset) | Public URL prefix for stored objects |
| `S3_PRESIGN_EXPIRY` | `3600` | Presigned URL lifetime in seconds (minimum 1) |
| `AWS_ACCESS_KEY_ID` | unset (empty = unset) | S3 access key |
| `AWS_SECRET_ACCESS_KEY` | unset (empty = unset) | S3 secret key |
| `ALLOWED_MIME_TYPES` | unset (server default) | Comma-separated list of accepted upload MIME types |
| `MAX_UPLOAD_SIZE` | `52428800` (50 MiB) | Maximum upload size in bytes |
| `MAX_AVATAR_SIZE` | `5242880` (5 MiB) | Maximum avatar size in bytes |
| `MAX_EMOJI_SIZE` | `262144` (256 KiB) | Maximum custom emoji size in bytes |

## Voice

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `STUN_SERVER` | `stun:stun.l.google.com:19302` | STUN server given to clients. Point it at your own coturn when you run TURN |
| `TURN_SERVER` | unset | TURN server URL |
| `TURN_USERNAME` | unset | Static TURN username |
| `TURN_CREDENTIAL` | unset | Static TURN credential |
| `TURN_SHARED_SECRET` | unset | Shared secret for time-limited TURN credentials (coturn `use-auth-secret`) |
| `TURN_CREDENTIAL_TTL` | `3600` | Lifetime of generated TURN credentials in seconds |
| `VOICE_UDP_PORT_MIN` | unset (ephemeral ports) | Lowest UDP port the voice SFU may use for peer connections. Must be set together with `VOICE_UDP_PORT_MAX`; the server refuses to start if only one is set or if min > max. Open/forward this UDP range in your firewall. The shipped compose file sets it from `RTP_PORT_MIN` (default `10000`) |
| `VOICE_UDP_PORT_MAX` | unset (ephemeral ports) | Highest UDP port of the range. Each port in the range serves peer connections. Size it generously: each participant uses two peer connections (publisher and subscriber), and the ICE stack binds one UDP socket per usable local interface address (IPv4/IPv6; more under hostNetwork) per connection. You need at least 2 × concurrent participants × local addresses, plus headroom. Example: 10 participants on a host with 2 addresses need at least 40 ports; 100 is a sensible choice. If the range is too small, joining voice fails under load. Ports must be 1-65535. The shipped compose file sets it from `RTP_PORT_MAX` (default `10100`) |
| `VOICE_ICE_INTERFACES` | unset (all interfaces) | Comma-separated list of network interface names allowed for voice ICE candidates (e.g. the public interface, `eth0`). Names are exact and case-sensitive; a name that matches no interface on the host means no ICE candidates, so voice will fail (a warning is logged at startup). Other interfaces (VPN, container bridges) are not advertised to clients, and filtering also reduces the number of ports consumed from the `VOICE_UDP_PORT_MIN`-`VOICE_UDP_PORT_MAX` range |
| `VOICE_ICE_IP_FAMILY` | `both` | IP family for voice ICE candidates: `v4`, `v6` or `both` (case-insensitive). Invalid values fail startup. Restricting the family also reduces ports consumed from the UDP range. If `PUBLIC_IP` is of the excluded family it is ignored (a warning is logged) |

## Observability

| Variable | Required / default | Purpose |
|----------|--------------------|---------|
| `OBSERVABILITY_ENABLED` | `false` | Enable OpenTelemetry traces and metrics export |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP collector endpoint |
| `OTEL_SERVICE_NAME` | `vc-server` | Service name reported in telemetry |
| `OTEL_TRACES_SAMPLER_ARG` | `0.1` | Trace sampling ratio (0.0 to 1.0) |
| `RUST_LOG` | `vc_server=info` | Log filter |
| `GRAFANA_URL` | unset (empty = unset) | Grafana URL shown in the admin UI |
| `TEMPO_URL` | unset (empty = unset) | Tempo URL |
| `LOKI_URL` | unset (empty = unset) | Loki URL |
| `PROMETHEUS_URL` | unset (empty = unset) | Prometheus URL |

## Other variables

These are read outside `server/src/config.rs`:

| Variable | Default | Read in | Purpose |
|----------|---------|---------|---------|
| `PUBLIC_URL` | unset | `auth/login.rs` | Public URL of the server; required for the OIDC browser login flow (callback is `$PUBLIC_URL/auth/oidc/callback`) |
| `AWS_REGION` | `us-east-1` | `chat/s3.rs` | S3 region |
| `DEPLOYMENT_ENVIRONMENT` | `local` | `observability/` | Environment label on telemetry |
| `RATE_LIMIT_TRUSTED_PROXIES` | unset (trust none) | `ratelimit/config.rs` | Comma-separated CIDRs or bare IPs of reverse proxies whose `X-Forwarded-For` / `X-Real-IP` headers are trusted for rate limiting, e.g. `10.42.0.0/16` (k3s pod network) or your compose network subnet. Requests from other peers use the peer address. Invalid entries are skipped with a startup warning |
| `RATE_LIMIT_TRUST_PROXY` | - | `ratelimit/config.rs` | **Deprecated, ignored.** Setting it to `true` logs a startup warning and trusts nothing; set `RATE_LIMIT_TRUSTED_PROXIES` instead |
| `RATE_LIMIT_*` | see [rate-limiting.md](rate-limiting.md) | `ratelimit/config.rs` | Rate limiting |

