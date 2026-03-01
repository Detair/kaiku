# Observability Runbook

**Stack:** OpenTelemetry → Tempo (traces) · Prometheus (metrics) · Loki (logs) · Sentry (errors)
**Last reviewed:** 2026-02-27

---

## Overview

Kaiku's observability pipeline routes all signals through a single OTel Collector instance so that tooling, sampling policies, and PII scrubbing are applied in one place.

```
vc-server (OTLP)
  │
  ▼
otel-collector:4317 (gRPC) / :4318 (HTTP)
  │
  ├─── traces ──────────► Tempo :4317
  │         │
  │         └─ spanmetrics connector
  │                       │
  ├─── metrics ◄──────────┘ (RED metrics)
  │         └────────────► Prometheus :8889 (scrape)
  │
  └─── logs ───────────► Loki :3100
                          │
                          └─ linked from Sentry via trace_id
```

### Instrumentation summary

| Signal | Source | Retention |
|--------|--------|-----------|
| Traces | vc-server (`tracing` + `opentelemetry` crates) | 30 days (Tempo) |
| RED metrics | spanmetrics connector (derived from traces) | 90 days (Prometheus TSDB) |
| Application metrics | vc-server Prometheus endpoint `:9090` | 90 days |
| Structured logs | vc-server (`tracing-subscriber` OTLP layer) | 14 days (Loki) |
| Crash / exception | Sentry SDK (client + server) | 90 days |

---

## Dashboard Reference

| Dashboard | Location | Purpose |
|-----------|----------|---------|
| **RED Metrics** | Grafana › Kaiku › API RED | Rate, Error, Duration for all HTTP routes |
| **Voice Latency** | Grafana › Kaiku › Voice | Join latency P50/P95/P99, active sessions |
| **Client Crash Rate** | Grafana › Kaiku › Client | Tauri crash rate, Sentry DSN breakdown |
| **Log Volume** | Grafana › Loki › Log Explorer | Log volume by level/service, error spikes |

> Grafana URL: `http://<host>:3000` — default org, folder **Kaiku**.

---

## Alert Runbook Entries

### `APIHighErrorRate` (warning)

**Meaning:** More than 0.1% of API spans are returning `STATUS_CODE_ERROR` over a rolling 5-minute window.

**Immediate triage:**
1. Open Sentry → Issues → sort by "First Seen" in the last 30 minutes.
2. Find a representative `trace_id` in the Sentry event detail.
3. Paste `trace_id` into Tempo (Grafana › Explore › Tempo › TraceQL: `{ .trace_id = "<id>" }`).
4. Identify which handler/span is the root cause.
5. Check Loki for correlated `ERROR` log lines: `{service="vc-server"} |= "ERROR" | json`.

**Escalation:** If error rate > 1% or rising, escalate to on-call engineer. Silence after root cause identified and a fix deployed.

---

### `APIP99LatencyHigh` (warning)

**Meaning:** The 99th-percentile API response duration has been above 200ms for 5 minutes.

**Immediate triage:**
1. Check RED dashboard — which `http.route` has the highest P99?
2. In Tempo, run: `{ duration > 200ms }` — sort descending by duration.
3. Examine the slowest traces: identify database, Valkey, or downstream RPC calls.
4. Check PostgreSQL slow query log: `docker compose logs postgres | grep duration`.
5. If a single route dominates, check for missing index or N+1 query pattern.

**Escalation:** If P99 > 500ms or affecting > 10% of requests, escalate to engineering. Apply caching or query optimisation as appropriate.

---

### `VoiceJoinLatencyHigh` (critical)

**Meaning:** The 95th-percentile latency for voice-join spans has exceeded 500ms for 2 minutes. The steady-state end-to-end target is <50ms; this threshold is the _join_ sequence (ICE + DTLS), not media delivery.

**Immediate triage:**
1. Check server CPU/memory: `docker stats canis-server`.
2. Verify STUN/TURN reachability: `ping <STUN_SERVER>`.
3. Check WebRTC SFU port availability: `ss -ulnp | grep 10[0-9][0-9][0-9][0-9]`.
4. In Tempo: `{ span.name =~ ".*voice.*join.*" && duration > 500ms }` — look for ICE negotiation delays.
5. Check active voice sessions in Valkey: `docker exec canis-valkey valkey-cli keys "voice:*"`.

**Escalation:** If > 50% of joins are slow, set server maintenance mode and page on-call immediately.

---

### `APIErrorBudgetFastBurn` (critical)

**Meaning:** The API error rate over the last hour is 14.4× the SLO budget (0.001). At this rate the 30-day error budget will be exhausted in ~2 hours.

**Immediate triage:**
1. This alert fires alongside `APIHighErrorRate` — follow that runbook first.
2. Identify the blast radius: single route vs. global degradation.
3. If a recent deployment is correlated: initiate rollback via `git revert` + redeploy.
4. Open Sentry Release health — compare error rates before/after the current release.
5. If not deployment-related: check infrastructure — PostgreSQL, Valkey, network partition.

**Escalation:** Page engineering lead immediately. Open a P1 incident in the incident tracker.

---

### `ObservabilityCollectorDown` (critical)

**Meaning:** Prometheus cannot reach the OTel Collector scrape endpoint (`otel-collector:8889`). All derived metrics (spanmetrics RED) and potentially all trace/log shipping are affected — you are flying blind.

**Immediate triage:**
1. `docker compose ps otel-collector` — check container status.
2. If stopped: `docker compose --profile monitoring up -d otel-collector`.
3. If running but unhealthy: `docker compose logs otel-collector --tail 100` — look for pipeline errors.
4. Validate config: `docker exec canis-otel-collector /otelcol-contrib --config /etc/otelcol-contrib/config.yaml --check`.
5. Confirm vc-server is still exporting OTLP: check server logs for `OpenTelemetry exporter error`.
6. Restore collector first; then verify spanmetrics appear in Prometheus within 15s (flush interval).

**Escalation:** If collector cannot start within 15 minutes, fall back to application-level Prometheus metrics on `:9090` and file an incident.

---

## OTel Collector Health Check Commands

```bash
# Check container status
docker compose --profile monitoring ps otel-collector

# Tail live logs (pipeline errors, exporter backpressure)
docker compose --profile monitoring logs -f otel-collector

# Validate config syntax without starting
docker run --rm \
  -v "$PWD/infra/monitoring/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro" \
  otel/opentelemetry-collector-contrib:latest \
  --config /etc/otelcol-contrib/config.yaml --check

# Query Prometheus for collector self-metrics (requires collector running)
curl -s http://localhost:8889/metrics | grep otelcol_

# Check pipeline drop rates
curl -s http://localhost:8889/metrics \
  | grep -E 'otelcol_(processor|exporter)_.*dropped'

# Verify spanmetrics are flowing
curl -s http://localhost:8889/metrics \
  | grep traces_span_metrics_calls_total \
  | head -5
```

---

## Sampling and Cardinality Tuning Guide

### Trace sampling

The current collector config uses **no head-based sampling** (all spans are forwarded to Tempo). To add probabilistic sampling, insert a `probabilistic_sampler` processor in the traces pipeline:

```yaml
processors:
  probabilistic_sampler:
    sampling_percentage: 10   # keep 10% of traces
```

For tail-based sampling (keep all errors, sample normal traffic), add the `tail_sampling` processor (requires a single collector instance or aggregator):

```yaml
processors:
  tail_sampling:
    decision_wait: 10s
    policies:
      - name: errors-policy
        type: status_code
        status_code: { status_codes: [ERROR] }
      - name: slow-traces-policy
        type: latency
        latency: { threshold_ms: 500 }
      - name: probabilistic-policy
        type: probabilistic
        probabilistic: { sampling_percentage: 5 }
```

### Metric cardinality

High cardinality arises from dimensions with unbounded values (user IDs, request IDs). The `attributes/metrics_pii` processor already deletes `user_id`, `session_id`, and `http.url`. If Prometheus reports memory pressure:

1. Identify high-cardinality series: `topk(20, count by (__name__)({__name__=~".+"}))`.
2. Add additional delete actions in `attributes/metrics_pii`.
3. Consider replacing `http.route` with a bucketed route pattern if parameterised routes appear as individual label values.
4. Review spanmetrics dimensions: remove dimensions that are not used in dashboards or alert rules.

### Log volume tuning

If Loki ingest cost is high:
- Filter out `DEBUG`-level logs at the collector before export by adding a `filter` processor to the logs pipeline:
  ```yaml
  processors:
    filter/logs_level:
      logs:
        exclude:
          match_type: strict
          severity_texts: ["DEBUG", "TRACE"]
  ```
- Insert `filter/logs_level` before `batch` in the logs pipeline.
