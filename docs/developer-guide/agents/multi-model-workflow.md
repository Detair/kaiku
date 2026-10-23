# Multi-Model Agent Workflow

Decided 2026-10-22 for the v1.0 Family Release
(`docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`).

## Roles

| Role | Model | Why (initial assumption — recalibrate after the first review round) |
|------|-------|------|
| **Builder** | Claude Opus 5.5 | Implementation, planning, consolidating findings |
| **Primary reviewer** | Claude Fable 5.1 | Deepest reasoning: adversarial security, concurrency/correctness, final verification of every Blocker |
| **External reviewer (mandatory)** | Gemini 4 Argon (`gemini-4-argon-high`) via `agy` (Antigravity CLI) | Independent second opinion from a different model family; very long context → whole-codebase sweeps (UI consistency, hardcoded strings, cross-file drift); Google-stack depth (Android, Kotlin/Compose, FCM/Firebase) |
| **External reviewer (mandatory)** | GPT-6-Astra (`gpt-6-astra`, effort `max`) via `codex exec -s read-only` | OpenAI's frontier model; read-only sandbox can run grep/git, so it verifies claims against the tree; third independent model family |
| **External reviewer (optional)** | Kimi K3 / Qwen 3.8 Max / GLM 5.3 via `opencode run --agent plan` (OpenCode Go subscription) | Further model families for cheap cross-checks (e.g. Blocker verification) and fallback when Codex hits its usage limit. DeepSeek V4 Pro needs the "Global regions" workspace setting (maintainer's call) |

**Rule:** every review — lane review or PR review — gets at least one
external-model pass (Gemini or Codex) in addition to Fable. Always use each
provider's strongest model: `gemini-4-argon-high`, `gpt-6-astra` at `max`
(`ultra` adds autonomous delegation — avoid for controlled reviews). Findings agreed by both carry more
weight; disagreements are resolved by re-reading the code, not by vote.

## Model efficiency (maintainer rule, 2026-10-22)

Pick the cheapest Claude model that fits the role:

| Work | Model |
|---|---|
| Mechanical implementation (plan gives files, signatures, tests; 1–3 files; CI/scripts) | Sonnet 5.5 |
| Integration/design implementation (multi-file, architecture, concurrency, security code) | Opus 5.5 |
| Task review of small/medium diffs, every scoped re-review | Sonnet 5.5 |
| Review of security/concurrency code, large diffs, final whole-branch review per stream | Fable 5.1 |
| Fix-loop escalation (round 4–5) | one tier above the stuck implementer |

The external-model pass stays mandatory for every task review.

## Experts (consultable by any agent)

Experts answer focused questions; they do not review whole diffs. Ask one
when a decision depends on deep domain knowledge.

| Expert | Model | Domain | How to ask |
|--------|-------|--------|------------|
| `expert-security` | Fable | Threat modelling, auth/session/OIDC, SSRF, crypto use, supply chain | Agent tool: `subagent_type: expert-security` |
| `expert-webrtc-voice` | Fable | webrtc-rs SFU, dual PeerConnection, Opus/VP8, TURN/ICE, cpal audio | Agent tool: `subagent_type: expert-webrtc-voice` |
| `expert-rust-backend` | Opus | axum/tokio/sqlx/Valkey patterns, migrations, Kaiku module standards | Agent tool: `subagent_type: expert-rust-backend` |
| `android-push` | Gemini Argon | Android lifecycle/doze, FCM + UnifiedPush, Compose, Play Services | `scripts/agents/ask-gemini-expert.sh android-push "<question>"` |
| `release-ci` | GPT-6-Astra | GitHub Actions, Tauri signing/updater, Authenticode/notarization, Android release builds | `scripts/agents/ask-codex-expert.sh release-ci "<question>"` |
| `ui-design-system` | Gemini Argon | Kaiku design tokens/contrast rules, Solid.js component consistency, i18n readiness | `scripts/agents/ask-gemini-expert.sh ui-design-system "<question>"` |

Any brief can be asked through any script (`ask-gemini-expert.sh`,
`ask-codex-expert.sh`, `ask-opencode-expert.sh`) for a second opinion. Gemini experts run read-only (`--mode plan`, no shell) with the repo as working
directory. Briefs live in `docs/developer-guide/agents/experts/`.

## Quota notes

- Codex (ChatGPT plan) is shared with `openai/*` models in OpenCode. Six
  parallel `gpt-6-astra`/`max` lane reviews exhausted it in one go
  (2026-10-22) — run Codex lanes sequentially and reserve `max` for
  verification passes.
- Headless `agy` auto-denies shell commands; prompts must tell Gemini to use
  its built-in read/search tools only.
- Gemini via `agy` has an individual quota: the 2026-10-22 lane review (6 large
  runs) plus experts exhausted it for ~137 h. Use OpenCode Go models (Kimi K3
  first) as the external reviewer while Gemini/Codex are limited; reserve
  Gemini for whole-codebase sweeps where its context length matters.
- 2026-10-23 18:10: Gemini 4 Argon exhausted until ~2026-10-30 (maintainer). Other
  Antigravity models work and have separate quota: `gemini-3.8-flash-high` (Google,
  fast, good for small/medium diffs and re-reviews) and `gpt-oss-120b-medium`
  (OpenAI open-weight, another family). Same headless rule (no shell; built-in
  read/search tools only). External-review routing now: small/medium diffs →
  Gemini 3.8 Flash or GPT-OSS 120B (no lock); security/concurrency/large →
  Codex (`flock` + `timeout 2400`); Kimi when OpenCode Go has quota.
- A Codex run once hung for ~7 h holding the shared lock and blocked all
  sessions: always wrap Codex in `timeout 2400`.
