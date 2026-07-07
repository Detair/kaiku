# Kaiku load & stress harness (k6)

Reusable [k6](https://k6.io) scenarios for load- and stress-testing the server.
Each gap feature adds one scenario for its new endpoints; every scenario shares
the same auth helper, metrics, and pass/fail thresholds.

## Install k6

k6 is a single standalone binary — no toolchain added to the repo.

```bash
# Arch / CachyOS
sudo pacman -S k6           # or: yay -S k6-bin
# Debian/Ubuntu
sudo gpg -k && \
  curl -s https://dl.k6.io/key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/k6-archive-keyring.gpg && \
  echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list && \
  sudo apt-get update && sudo apt-get install k6
# Or grab the release binary: https://github.com/grafana/k6/releases
```

## Run

```bash
# 1) Validate the harness itself (do this first).
BASE_URL=http://localhost:8080 k6 run stress/scenarios/smoke.js

# 2) A feature scenario.
BASE_URL=http://localhost:8080 k6 run stress/scenarios/reaction-roles.js

# Tune the load.
VUS=50 DURATION=2m BASE_URL=http://localhost:8080 k6 run stress/scenarios/<name>.js
```

### Environment variables

| Var | Default | Meaning |
|---|---|---|
| `BASE_URL` | `http://localhost:8080` | Server under test |
| `VUS` | `10` | Virtual users (concurrency) |
| `DURATION` | `30s` | Run length |
| `LOAD_USER` / `LOAD_PASS` | `k6_load_test` / … | Dedicated load-test account (created if registration is open) |

## Reading results

Thresholds (in `lib/config.js`) are the pass/fail gate — k6 exits non-zero if any fails:

- **`server_errors` < 1%** — rate of 5xx. This is the real failure signal.
- **`http_req_duration` p95 < 800ms** — latency budget.
- **`checks` > 95%** — scenario assertions passing.

A **429** is *not* a failure — it's the rate limiter correctly shedding load. It's
tracked separately as `rate_limited` (informational). A healthy stress result is
"low 5xx, bounded p95, 429s appearing only once you exceed the configured
limits."

While a run is in flight, watch the **admin Command Center** (Settings → Admin →
Command Center): CPU %, memory, p95 latency, and error rate should stay bounded.
A query-count / N+1 regression shows up as **latency climbing with VUs** while
throughput plateaus.

## ⚠️ Running against the beta

The beta (`kaiku.pmind.de`) shares a host with game servers and enforces per-IP
and per-user rate limits. So:

- Use a **dedicated** `LOAD_USER`, never a real account.
- Keep beta runs **short and bounded** (low `VUS`, ≤30s) — enough to sanity-check
  a deploy, not to saturate.
- Do **heavy** runs against a **local** server (`docker-compose.dev` + a local
  build), where you can push high `VUS` without impacting the beta or tripping
  shared rate limits.

## Layout

```
stress/
  lib/
    config.js    # BASE_URL, load shape, thresholds
    metrics.js   # server_errors (5xx) vs rate_limited (429)
    auth.js      # authenticate() -> bearer token (use in setup())
  scenarios/
    smoke.js     # validates the harness (run first)
    _template.js # copy per feature
```
