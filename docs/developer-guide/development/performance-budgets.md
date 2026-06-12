# Performance Budgets

> Phase 8 reliability gate (Goal 5 item 2). Budgets exist to catch
> order-of-magnitude regressions in CI — lost indexes, N+1 queries,
> dependencies landing in the startup-critical path — not to micro-optimize.
> Raise a budget only in a PR that explains the cost.

## Enforced in CI

| Budget | Threshold | Gate | Baseline (2026-06-12) |
|---|---|---|---|
| Initial JS payload (gzipped, all assets referenced by `index.html`) | ≤ 300 KB | `scripts/check_bundle_budget.py` (Frontend job, after build) | 237 KB |
| Initial CSS payload (gzipped) | ≤ 25 KB | same | 16 KB |
| Largest entry chunk (gzipped) | ≤ 170 KB | same | 130 KB |
| Message history page (50 of 1,000 msgs) | best-of-3 < 500 ms | `performance_budgets.rs` (Rust Tests job) | ~5 ms local |
| Guild full-text search (1,000 msgs) | best-of-3 < 750 ms | same | ~10 ms local |

Bundle budgets are the practical proxy for the **<3 s startup** target:
lazy chunks don't block startup, so only the `index.html`-referenced
payload is counted. Server budgets use best-of-3 timing with generous
thresholds to stay deterministic on shared CI runners.

## Not yet automatable (manual / future instrumentation)

| Target (CLAUDE.md / roadmap) | Why not in CI | How to measure today |
|---|---|---|
| Client startup < 3 s | Needs a real Tauri window; headless CI can't render | Stopwatch on a release build; Tauri instrumentation later |
| Voice join < 2 s | Needs real WebRTC peers + SFU media path | Beta server smoke test with two clients |
| Client RAM idle < 80 MB / < 50 MB per 1,000 messages | OS-level process measurement of a running app | `ps`/Task Manager against a release build |
| Message list render (50 items) < 100 ms | jsdom timing is noise, not signal | Browser devtools profiling on the real app |

When the desktop client gains startup instrumentation (e.g. a timing event
at first-paint), the startup and render budgets should migrate into CI.
