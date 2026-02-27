# Incident Triage — Observability Decision Tree

**Purpose:** Step-by-step guide for on-call engineers to triage alerts and service degradation using Kaiku's observability stack.
**Prerequisite:** Monitoring stack running (`docker compose --profile monitoring up -d`).

---

## Triage Decision Tree

```
Alert fires
     │
     ▼
Step 1: Check Sentry for error spike
     │
     ├─► New errors? ──Yes──► Note trace_id from Sentry event ──► Step 2
     │
     └─► No errors ─────────► Go to Step 3 (latency/SLO metric issue)
```

---

## Step 1 — Check Sentry for Matching Error Spike

**Goal:** Determine if the alert is driven by unhandled exceptions or 5xx responses.

1. Open Sentry → **Issues** → filter **Last 1 hour** → sort by **Events**.
2. Look for a spike coinciding with the alert firing time.
3. Click into the top issue. Record:
   - `trace_id` from the "Additional Data" or tags panel (key: `trace_id`)
   - Affected `http.route` or function name
   - First-seen time vs. alert start time (correlation check)
4. If no Sentry errors but alert is firing → **go to Step 3**.

> **Tip:** If `ObservabilityCollectorDown` fired, Sentry may still receive data via its own SDK — check there first even if Prometheus/Tempo are dark.

---

## Step 2 — Find Correlated Trace in Tempo via trace_id

**Goal:** Understand the full call graph of a failing or slow request.

1. Open Grafana → **Explore** → data source: **Tempo**.
2. Query mode: **TraceQL**. Enter:
   ```
   { .trace_id = "<trace_id_from_sentry>" }
   ```
   Or search by service + time window:
   ```
   { resource.service.name = "vc-server" && duration > 200ms }
   ```
3. In the trace waterfall:
   - Identify the **first red/errored span** (root cause span, not propagated error).
   - Note the span name, attributes (`db.statement`, `http.url`, `rpc.method`).
   - Look for spans with unexpectedly long duration relative to siblings.
4. Correlate to logs (Loki):
   - Click **Logs for this span** in Tempo (uses `trace_id` → Loki derived field).
   - Or in Loki: `{service="vc-server"} | json | trace_id = "<id>"`.
5. Note the **root cause component**: DB query, Valkey call, downstream service, or application logic.
6. Proceed to **Step 3** to cross-check metrics, or jump to **Escalation** if root cause is clear.

---

## Step 3 — Check Metric Cardinality and RED Metrics in Prometheus

**Goal:** Quantify the blast radius and verify the metric signal is reliable.

1. Open Grafana → **Explore** → data source: **Prometheus**.

2. **Check error rate:**
   ```promql
   sum by (http_route) (
     rate(traces_span_metrics_calls_total{status_code="STATUS_CODE_ERROR"}[5m])
   )
   / sum by (http_route) (
     rate(traces_span_metrics_calls_total[5m])
   )
   ```
   → Identify which routes are contributing most to the error ratio.

3. **Check P99 latency by route:**
   ```promql
   histogram_quantile(0.99,
     sum by (le, http_route) (
       rate(traces_span_metrics_duration_milliseconds_bucket[5m])
     )
   )
   ```
   → Compare against baseline (normal P99 < 100ms for most routes).

4. **Check cardinality health** (prevent Prometheus OOM):
   ```promql
   topk(20, count by (__name__)({__name__=~".+"}))
   ```
   → If any series count is unexpectedly high (> 10k), investigate label explosion. Check `http.url` or `user_id` leaking into metrics (the PII processor should prevent this — see collector logs).

5. **Verify scrape health:**
   ```promql
   up{job="otel-collector"}
   up{job="vc-server"}
   ```
   → If `0`, collector or server metrics endpoint is down → **Step 4**.

---

## Step 4 — Check Collector Health

**Goal:** Ensure the observability pipeline itself is not the source of missing data.

1. **Container status:**
   ```bash
   docker compose --profile monitoring ps otel-collector
   ```

2. **Live logs** (look for exporter errors, pipeline drops, config issues):
   ```bash
   docker compose --profile monitoring logs -f otel-collector --tail 200
   ```
   Common errors:
   | Log pattern | Meaning | Action |
   |-------------|---------|--------|
   | `connection refused tempo:4317` | Tempo not running | Start Tempo container |
   | `connection refused loki:3100` | Loki not running | Start Loki container |
   | `sending queue is full` | Downstream exporter slow | Check exporter target health |
   | `context deadline exceeded` | Batch timeout | Verify network between collector and exporters |

3. **Self-metrics** (collector instrumentation):
   ```bash
   curl -s http://localhost:8889/metrics | grep -E 'otelcol_(processor|exporter).*dropped'
   ```
   → Non-zero `dropped_spans` or `dropped_metric_points` indicates backpressure.

4. **Restart if needed:**
   ```bash
   docker compose --profile monitoring restart otel-collector
   ```
   → Allow 30s for pipelines to reconnect and flush interval (15s) to pass.

5. If collector health is confirmed OK → return to **Step 2/3** with fresh data.
   If collector cannot recover → follow `ObservabilityCollectorDown` runbook in `observability-runbook.md`.

---

## Step 5 — Escalation Matrix

### Severity classification

| Condition | Classification | Response SLA |
|-----------|---------------|-------------|
| Warning alert, error rate < 1%, P99 < 500ms | **Warning** | Investigate within 30 min |
| Critical alert, or error rate > 1% | **Critical** | Page on-call immediately |
| Error budget fast-burn (14.4×) | **P1 Incident** | Page on-call + engineering lead within 5 min |
| Voice join P95 > 500ms | **Critical** | Page on-call immediately |
| Collector down > 5 min | **Critical** | Page on-call, restore observability first |
| Multiple critical alerts simultaneously | **Major Incident** | Incident commander + full team |

### Escalation path

```
Warning
  └─► On-call engineer self-resolves
        └─► If unresolved in 30 min → escalate to Critical

Critical
  └─► Page on-call engineer (PagerDuty / alertmanager webhook)
        └─► Acknowledge within 10 min
        └─► If unresolved in 30 min → page engineering lead

P1 / SLO Breach
  └─► Page on-call + engineering lead simultaneously
        └─► Open incident channel: #incident-YYYY-MM-DD
        └─► Assign incident commander
        └─► Status page update within 15 min
        └─► Post-mortem within 48h

ObservabilityCollectorDown (Critical)
  └─► Restore collector FIRST (blind operation otherwise)
        └─► Use Sentry as fallback error signal while collector is down
        └─► Check application logs directly: docker compose logs server
```

### Communication checklist (P1 incidents)

- [ ] Incident channel created: `#incident-YYYY-MM-DD`
- [ ] Incident commander assigned
- [ ] Engineering lead notified
- [ ] Status page updated
- [ ] Customer-facing impact assessed
- [ ] Mitigation applied or rollback initiated
- [ ] All-clear posted to incident channel
- [ ] Post-mortem scheduled

---

## Quick Reference: Key URLs

| Tool | URL | Purpose |
|------|-----|---------|
| Grafana | `http://<host>:3000` | Dashboards, Explore (Tempo/Loki/Prometheus) |
| Prometheus | `http://<host>:9090` | Raw metrics, alert state |
| Sentry | `https://sentry.io/<org>/kaiku/` | Error tracking, release health |
| OTel Metrics | `http://localhost:8889/metrics` | Collector self-metrics (local only) |

---

## Glossary

| Term | Definition |
|------|-----------|
| **RED metrics** | Rate, Error, Duration — the three key service health signals |
| **spanmetrics** | OTel Collector connector that generates RED metrics from trace spans |
| **trace_id** | W3C Trace Context identifier linking spans across services |
| **error budget** | Allowed downtime/errors before SLO is breached (99.9% → 0.1% errors/month) |
| **fast burn** | Error rate high enough to exhaust budget in <2 hours (14.4× normal rate) |
| **P95 / P99** | 95th / 99th percentile latency — worst-case experience for 5% / 1% of requests |
