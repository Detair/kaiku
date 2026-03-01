# Observability Contract

**Date:** 2026-02-27
**Status:** Active
**Scope:** vc-server (Axum 0.8 + Tokio), vc-client (Tauri 2.0 + Solid.js)
**Signal model:** OTel traces + metrics + logs via OTLP → Grafana Alloy → Tempo / Mimir / Loki
**Error tracking:** Sentry (server + native client)
**Design ref:** [2026-02-15-phase-7-a11y-observability-design.md](../plans/2026-02-15-phase-7-a11y-observability-design.md)
**Architecture ref:** [2026-02-15-opentelemetry-grafana-reference-design.md](../plans/2026-02-15-opentelemetry-grafana-reference-design.md)

---

## 1. Purpose

This document is the authoritative specification for how Kaiku emits, labels, and protects telemetry signals. Every instrumentation decision must be consistent with the rules here. Violations are classified as **ERROR** (blocks release) or **WARNING** (tracked, must be resolved within one sprint).

---

## 2. Resource Attributes

Resource attributes identify the origin of every signal. They are set once at SDK initialization and attached to all spans, metrics, and log records from that process.

### 2.1 Required — Server (`vc-server`)

| Attribute | Type | Example | Notes |
|-----------|------|---------|-------|
| `service.name` | string | `vc-server` | Constant. Never dynamic. |
| `service.version` | string | `0.9.1` | From `CARGO_PKG_VERSION` at build time. |
| `service.namespace` | string | `kaiku` | Constant. |
| `deployment.environment` | string | `production` | One of: `development`, `staging`, `production`. |
| `host.name` | string | `node-01` | Hostname of the running instance. |
| `process.runtime.name` | string | `tokio` | Constant. |
| `process.runtime.version` | string | `1.36.0` | Tokio version from `CARGO_PKG_VERSION`. |

### 2.2 Required — Native Client (`vc-client`, Tauri 2.0)

| Attribute | Type | Example | Notes |
|-----------|------|---------|-------|
| `service.name` | string | `vc-client` | Constant. |
| `service.version` | string | `0.9.1` | From Tauri `tauri.conf.json` version field. |
| `service.namespace` | string | `kaiku` | Constant. |
| `deployment.environment` | string | `production` | Same enum as server. |
| `os.type` | string | `linux` | Lowercase. From `std::env::consts::OS`. |
| `os.version` | string | `6.6.0` | Kernel version string. |
| `process.runtime.name` | string | `tauri` | Constant. |

### 2.3 Required — Frontend (`vc-client` browser context, Solid.js)

| Attribute | Type | Example | Notes |
|-----------|------|---------|-------|
| `service.name` | string | `vc-client-web` | Constant. |
| `service.version` | string | `0.9.1` | Injected by Vite at build time via `import.meta.env.VITE_APP_VERSION`. |
| `service.namespace` | string | `kaiku` | Constant. |
| `deployment.environment` | string | `production` | Same enum as server. |
| `browser.user_agent` | string | (omitted) | **FORBIDDEN** — see Section 7. |

---

## 3. Span Naming Convention

Span names must be stable, low-cardinality, and human-readable. They follow the pattern `domain.operation` for internal operations and the OTel HTTP semantic convention for inbound HTTP.

### 3.1 HTTP Server Spans (Axum 0.8)

Use the **parameterised route template**, not the resolved URL. Axum's `MatchedPath` extractor provides this.

```
http.server.request
```

The route is recorded in the `http.route` span attribute, not the span name. This keeps span names stable regardless of path parameter values.

**Correct:**
```
span name:  http.server.request
http.route: /api/v1/guilds/{guild_id}/channels/{channel_id}
```

**Incorrect (ERROR):**
```
span name:  GET /api/v1/guilds/123/channels/456
```

### 3.2 Domain Operation Spans

Internal spans use `domain.verb_noun` format. All lowercase, underscores only.

| Domain | Example span names |
|--------|--------------------|
| `auth` | `auth.login`, `auth.token_refresh`, `auth.session_validate` |
| `chat` | `chat.message_send`, `chat.message_fetch`, `chat.attachment_upload` |
| `voice` | `voice.session_join`, `voice.session_leave`, `voice.rtp_forward` |
| `ws` | `ws.connection_open`, `ws.connection_close`, `ws.event_dispatch` |
| `db` | `db.query`, `db.transaction` (use OTel DB semantic conventions for attributes) |
| `crypto` | `crypto.olm_encrypt`, `crypto.olm_decrypt`, `crypto.megolm_encrypt` |
| `admin` | `admin.user_ban`, `admin.role_assign` |

### 3.3 Client Spans (Tauri commands)

Tauri command spans use `tauri.command_name` format, matching the registered command name exactly.

```
tauri.send_message
tauri.join_voice_channel
tauri.get_session
```

### 3.4 Frontend Spans (Solid.js)

Frontend spans are emitted only for user-initiated navigations and explicit async operations. They use `ui.domain_action` format.

```
ui.auth_login
ui.channel_switch
ui.voice_join
```

---

## 4. Span Attributes

### 4.1 Required on all spans

| Attribute | Type | Source |
|-----------|------|--------|
| `service.name` | string | Resource (inherited) |
| `deployment.environment` | string | Resource (inherited) |

### 4.2 Required on HTTP server spans

These follow [OTel HTTP semantic conventions](https://opentelemetry.io/docs/specs/semconv/http/http-spans/).

| Attribute | Type | Example |
|-----------|------|---------|
| `http.request.method` | string | `GET` |
| `http.route` | string | `/api/v1/guilds/{guild_id}` |
| `http.response.status_code` | int | `200` |
| `url.scheme` | string | `https` |
| `server.address` | string | `api.kaiku.example` |
| `network.protocol.version` | string | `1.1` |

**Forbidden on HTTP spans (ERROR):**
- `url.full` containing query strings with tokens or passwords
- `http.request.header.authorization`
- `http.request.header.cookie`

### 4.3 Required on database spans

These follow [OTel DB semantic conventions](https://opentelemetry.io/docs/specs/semconv/database/).

| Attribute | Type | Example |
|-----------|------|---------|
| `db.system` | string | `postgresql` |
| `db.name` | string | `kaiku` |
| `db.operation` | string | `SELECT` |
| `db.sql.table` | string | `messages` |

**Forbidden on database spans (ERROR):**
- `db.statement` containing user-supplied values (bind parameters only, never interpolated SQL)

### 4.4 Required on WebSocket spans

| Attribute | Type | Example |
|-----------|------|---------|
| `ws.event_type` | string | `ClientEvent::SendMessage` |
| `ws.outcome` | string | `ok` or `error` |

### 4.5 Required on voice spans

| Attribute | Type | Example |
|-----------|------|---------|
| `voice.channel_id` | string | `ch_01HX...` (opaque ID, not name) |
| `voice.session_outcome` | string | `connected`, `failed`, `timeout` |

**Forbidden on voice spans (ERROR):**
- Any attribute containing audio content, codec payloads, or DTLS keying material

---

## 5. Metric Naming Conventions

### 5.1 Format rules

- All metric names: `snake_case`
- Unit suffix required where the unit is not obvious from the name
- Namespace prefix: `kaiku_`
- No user IDs, guild IDs, or channel IDs in metric names (those belong in labels, subject to the allowlist in Section 6)

**Pattern:** `kaiku_{domain}_{noun}_{unit}` or `kaiku_{domain}_{noun}_{aggregation}`

### 5.2 Required server metrics

| Metric name | Type | Unit | Description |
|-------------|------|------|-------------|
| `kaiku_http_request_duration_ms` | Histogram | ms | Latency of HTTP handler execution. Buckets: `[5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000]`. |
| `kaiku_http_requests_total` | Counter | requests | Total HTTP requests, by method, route, and status code. |
| `kaiku_http_errors_total` | Counter | requests | HTTP 4xx/5xx responses. |
| `kaiku_ws_connections_active` | UpDownCounter | connections | Current open WebSocket connections. |
| `kaiku_ws_reconnects_total` | Counter | reconnects | WebSocket reconnection attempts. |
| `kaiku_ws_messages_total` | Counter | messages | WebSocket messages dispatched, by event type. |
| `kaiku_voice_joins_total` | Counter | joins | Total voice join attempts, by outcome (`success`, `failure`). |
| `kaiku_voice_sessions_active` | UpDownCounter | sessions | Current active voice sessions. |
| `kaiku_voice_session_duration_seconds` | Histogram | seconds | Duration of completed voice sessions. |
| `kaiku_voice_rtp_packets_forwarded_total` | Counter | packets | RTP packets forwarded by the SFU. |
| `kaiku_db_query_duration_seconds` | Histogram | seconds | SQLx query execution time. |
| `kaiku_db_pool_connections_active` | Gauge | connections | Active database pool connections. |
| `kaiku_db_pool_connections_idle` | Gauge | connections | Idle database pool connections. |
| `kaiku_auth_login_attempts_total` | Counter | attempts | Login attempts, by outcome (`success`, `failure`). |
| `kaiku_auth_token_refresh_total` | Counter | refreshes | Token refresh operations, by outcome. |
| `kaiku_otel_export_failures_total` | Counter | failures | OTLP export failures from the SDK. |
| `kaiku_otel_dropped_spans_total` | Counter | spans | Spans dropped due to queue overflow. |
| `kaiku_process_memory_bytes` | Gauge | bytes | Process resident set size (RSS) from /proc/self/status. |

### 5.3 Required client metrics (Tauri native)

| Metric name | Type | Unit | Description |
|-------------|------|------|-------------|
| `kaiku_client_tauri_command_duration_seconds` | Histogram | seconds | Tauri command execution time. |
| `kaiku_client_tauri_command_errors_total` | Counter | errors | Tauri command errors, by command name. |
| `kaiku_client_voice_webrtc_connect_duration_seconds` | Histogram | seconds | Time from join intent to ICE connected. |
| `kaiku_client_voice_webrtc_failures_total` | Counter | failures | WebRTC connection failures, by reason. |

### 5.4 Histogram bucket policy

Default buckets for latency histograms: `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]` seconds.

Voice-specific latency histograms use tighter buckets: `[0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5]` seconds, reflecting the 50ms latency target.

---

## 6. Low-Cardinality Label Allowlist

Labels (metric attributes) must come from this allowlist. Any label not listed here is **FORBIDDEN (ERROR)**.

The cardinality budget per metric is **100 unique label value combinations**. Exceeding this is a WARNING in staging and an ERROR in production.

### 6.1 Allowed labels

| Label key | Allowed values | Notes |
|-----------|---------------|-------|
| `deployment.environment` | `development`, `staging`, `production` | From resource. |
| `service.name` | `vc-server`, `vc-client`, `vc-client-web` | From resource. |
| `http.request.method` | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS` | HTTP method only. |
| `http.route` | Parameterised route template | e.g. `/api/v1/guilds/{guild_id}`. Never resolved. |
| `http.response.status_code` | Integer status code | Grouped to class (`2xx`, `4xx`, `5xx`) in dashboards. |
| `outcome` | `success`, `failure`, `timeout`, `cancelled` | Generic result label. |
| `error.type` | Short error category string | e.g. `db_timeout`, `auth_invalid_token`. Max 20 distinct values per metric. |
| `voice.session_outcome` | `connected`, `failed`, `timeout`, `disconnected` | Voice session result. |
| `ws.event_type` | Enum values from `ClientEvent` / `ServerEvent` in `vc-common` | Protocol event names only. |
| `db.operation` | `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `BEGIN`, `COMMIT`, `ROLLBACK` | SQL operation class. |
| `db.sql.table` | Table name | Schema-level name only. No dynamic suffixes. |
| `tauri.command` | Registered Tauri command name | From `client/src-tauri/src/lib.rs` command list. |
| `os.type` | `linux`, `windows`, `macos` | Client only. |

### 6.2 Explicitly forbidden labels (ERROR)

- `user_id`, `user.id`, or any user identifier
- `guild_id`, `channel_id`, `message_id`, or any entity ID
- `ip_address`, `remote_addr`, or any network address
- `session_token`, `access_token`, `refresh_token`
- `username`, `display_name`, `email`
- `url.full`, `url.path` (use `http.route` instead)
- Any label with unbounded cardinality (e.g. free-text fields, UUIDs)

---

## 7. Forbidden Fields

The following fields must never appear in any telemetry signal (spans, metrics, logs, Sentry events). Emitting them is an **ERROR** that blocks release.

### 7.1 PII

| Field category | Examples |
|----------------|---------|
| User identifiers | `user_id`, `username`, `display_name`, `email`, `phone` |
| Network identifiers | `ip_address`, `remote_addr`, `x_forwarded_for` |
| Device identifiers | `device_id`, `mac_address`, `hardware_id` |
| Location data | `latitude`, `longitude`, `geo_city`, `geo_country` |
| Message content | Any field containing chat message body, channel topic, or guild description |

### 7.2 Secrets and credentials

| Field category | Examples |
|----------------|---------|
| Auth tokens | `access_token`, `refresh_token`, `session_token`, `api_key` |
| Cryptographic material | Private keys, Olm session keys, Megolm group session keys, DTLS keying material |
| Passwords | `password`, `password_hash`, `bcrypt_hash` |
| Database credentials | `db_url`, `db_password`, connection string with credentials |
| Environment secrets | Any value from `.env` files tagged as secret |

### 7.3 High-cardinality fields

| Field category | Examples |
|----------------|---------|
| Resolved entity IDs | `user_id`, `guild_id`, `channel_id`, `message_id` as metric labels |
| Full URLs | `url.full` with query parameters |
| Stack traces as metric labels | Error message strings as label values |
| Request/response bodies | Any HTTP body content |
| Audio/media data | RTP payload, codec frames, audio samples |

---

## 8. Log Correlation Fields

All structured log records must include these fields. The server uses `tracing_subscriber` with JSON formatting; the client uses Tauri's logging plugin with the same schema.

### 8.1 Required fields on every log record

| Field | Type | Example | Source |
|-------|------|---------|--------|
| `timestamp` | RFC 3339 string | `2026-02-27T14:30:00.123Z` | `tracing_subscriber` |
| `level` | string | `INFO` | `tracing` level |
| `service.name` | string | `vc-server` | Resource attribute |
| `service.version` | string | `0.9.1` | Resource attribute |
| `deployment.environment` | string | `production` | Resource attribute |
| `trace_id` | hex string (32 chars) | `4bf92f3577b34da6a3ce929d0e0e4736` | W3C Trace Context |
| `span_id` | hex string (16 chars) | `00f067aa0ba902b7` | W3C Trace Context |
| `target` | string | `vc_server::auth::login` | Rust module path |
| `event` | string | `login_attempt` | Short event name |
| `outcome` | string | `success` | One of the outcome enum values |

### 8.2 Required fields on request-scoped log records

| Field | Type | Example | Notes |
|-------|------|---------|-------|
| `request_id` | UUID string | `01HX...` | Generated per request, propagated via `x-request-id` header. |
| `http.method` | string | `POST` | |
| `http.route` | string | `/api/v1/auth/login` | Parameterised template. |
| `http.status_code` | int | `200` | |
| `duration_ms` | float | `12.4` | Request handler duration. |

### 8.3 Required fields on security/audit log records

Security events (login, logout, permission change, ban, token revoke) must additionally include:

| Field | Type | Example | Notes |
|-------|------|---------|-------|
| `actor_id` | string | `usr_01HX...` | Opaque ID of the acting user. Allowed in logs, forbidden in metrics. |
| `action` | string | `user.ban` | Dot-separated domain.verb. |
| `target_type` | string | `user` | Entity type affected. |
| `policy_outcome` | string | `allowed` or `denied` | Permission check result. |

`actor_id` is permitted in logs for audit purposes but must never appear in metric labels or span attributes.

### 8.4 Forbidden log fields (ERROR)

- `password`, `password_hash`
- `access_token`, `refresh_token`, `session_token`
- Any Olm/Megolm key material
- `message_body`, `message_content`, or any field containing decrypted message text
- `ip_address` (use `request_id` for correlation instead)

---

## 9. Privacy Rules

### 9.1 Collector-level redaction (Grafana Alloy)

The Alloy pipeline must apply these processors before forwarding to any backend:

1. **Attribute redaction processor** strips any attribute matching these patterns from all signals:
   - `*.token*`, `*.secret*`, `*.password*`, `*.key*` (glob match on attribute name)
   - `http.request.header.authorization`
   - `http.request.header.cookie`
   - `http.request.header.x-api-key`

2. **Log body scrubbing** applies regex replacement before forwarding to Loki:
   - Pattern: `"(access_token|refresh_token|session_token)"\s*:\s*"[^"]*"` → `"$1":"[REDACTED]"`
   - Pattern: `Bearer\s+[A-Za-z0-9\-._~+/]+=*` → `Bearer [REDACTED]`

3. **Probabilistic sampling** for traces: 10% in production by default, 100% for error spans (always sample spans with `status.code = ERROR`).

### 9.2 SDK-level skip rules (server)

These rules apply in the OTel SDK configuration in `server/src/main.rs` before data leaves the process:

- Skip tracing for health check endpoints: `GET /health`, `GET /ready`, `GET /metrics`
- Skip tracing for static asset routes
- Do not record span events for voice RTP forwarding hot path (`voice.rtp_forward` spans record only start/end, no events)

### 9.3 Sentry `before_send` rules

Both the server Sentry SDK and the Tauri native Sentry SDK must register a `before_send` hook that:

1. Strips the `Authorization` header from all request contexts.
2. Strips the `Cookie` header from all request contexts.
3. Removes any event extra field whose key matches `*token*`, `*secret*`, `*password*`, or `*key*`.
4. Truncates `db.statement` values to 200 characters and removes bind parameter values.
5. Removes the `user.ip_address` field from the Sentry user context.
6. Sets `user.id` to the opaque `actor_id` only (never email or username).

The `before_send` hook must be tested with a unit test that asserts each of these fields is absent from the sanitized event.

### 9.4 Client-side rules (Solid.js frontend)

- No OTel SDK in the browser context by default. Frontend telemetry is limited to Sentry error tracking.
- Sentry in the browser must use the same `before_send` rules as the native client (items 1-6 above).
- No `console.log` calls in production builds (enforced by Vite build config).
- No telemetry calls inside E2EE message rendering paths.

---

## 10. Violation Severity Levels

### 10.1 ERROR (blocks release)

A release gate check must fail if any of these are detected in staging telemetry or static analysis:

| Violation | Detection method |
|-----------|-----------------|
| Forbidden field emitted in any signal | Alloy pipeline alert + integration test assertion |
| Span name contains a resolved path parameter (e.g. a UUID) | Regex check on span names in staging Tempo |
| Metric label not in the allowlist (Section 6) | Prometheus label cardinality alert |
| `before_send` hook absent or not tested | CI unit test gate |
| `trace_id` or `span_id` missing from a log record in a traced request | Log schema validation in integration tests |
| Resource attribute `service.name` missing or incorrect | OTel SDK startup assertion |
| OTLP export failure rate > 1% over 5 minutes | Alloy health metric alert |

### 10.2 WARNING (tracked, resolve within one sprint)

These do not block release but must be filed as issues and resolved before the following sprint ends:

| Violation | Detection method |
|-----------|-----------------|
| Metric cardinality exceeds 100 unique label combinations | Prometheus cardinality query |
| Span missing a required domain attribute (e.g. `http.route` on HTTP span) | Tempo attribute completeness check |
| Log record missing `request_id` on a request-scoped event | Log schema validation |
| Histogram bucket configuration deviates from Section 5.4 | Metric metadata check |
| Sentry event missing `service.name` tag | Sentry issue query |
| OTLP export queue depth > 80% of configured limit | Alloy queue metric alert |
| `deployment.environment` value not in the allowed enum | Resource attribute validation |

---

## Enforcement

Canonical implementation for server-side telemetry redaction and instrumentation enforcement lives in `server/src/observability/tracing.rs`. Any update to forbidden-attribute rules must update that file first and keep this contract aligned.

### 11.1 CI gates

The following checks run on every pull request:

- `cargo clippy` with `#[tracing::instrument]` presence verified on all public functions in `server/src/` (enforced via custom lint or doc check).
- Integration test suite asserts `trace_id` and `span_id` are present in log output for traced requests.
- Unit tests for `before_send` hooks in both server and client Sentry configurations.
- Metric name format check: all `kaiku_*` metric names must match `^kaiku_[a-z][a-z0-9_]*_(total|seconds|bytes|ratio|active|count|ms|idle)$`.

### 11.2 Staging validation

Before promoting a build to production:

- Run the Alloy pipeline smoke test: verify OTLP connectivity and that at least one trace, one metric, and one log record arrive in Tempo/Mimir/Loki within 60 seconds of startup.
- Query Tempo for span names matching `[A-F0-9]{8}-[A-F0-9]{4}` (UUID pattern) — any match is an ERROR.
- Query Prometheus for label cardinality on all `kaiku_*` metrics — any metric exceeding 100 series is a WARNING.

### 11.3 Ownership

| Area | Owner |
|------|-------|
| OTel SDK configuration | Server team |
| Alloy pipeline config | Infra team |
| Sentry `before_send` hooks | Server team + Client team |
| This document | Platform lead |

Changes to this document require a pull request with at least one review from the platform lead.

---

## 12. Changelog

| Date | Change |
|------|--------|
| 2026-02-27 | Initial version. |
