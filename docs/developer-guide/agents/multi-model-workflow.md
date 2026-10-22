# Multi-Model Agent Workflow

Decided 2026-10-22 for the v1.0 Family Release
(`docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`).

## Roles

| Role | Model | Why (initial assumption — recalibrate after the first review round) |
|------|-------|------|
| **Builder** | Claude Opus 5.5 | Implementation, planning, consolidating findings |
| **Primary reviewer** | Claude Fable 5.1 | Deepest reasoning: adversarial security, concurrency/correctness, final verification of every Blocker |
| **External reviewer (mandatory)** | Gemini 4 Argon (high) via `agy` | Independent second opinion from a different model family; very long context → whole-codebase sweeps (UI consistency, hardcoded strings, cross-file drift); Google-stack depth (Android, Kotlin/Compose, FCM/Firebase) |
| **External reviewer (planned)** | OpenAI Codex | Not installed yet; add when available |

**Rule:** every review — lane review or PR review — gets at least one
external-model pass in addition to Fable. Findings agreed by both carry more
weight; disagreements are resolved by re-reading the code, not by vote.

## Experts (consultable by any agent)

Experts answer focused questions; they do not review whole diffs. Ask one
when a decision depends on deep domain knowledge.

| Expert | Model | Domain | How to ask |
|--------|-------|--------|------------|
| `expert-security` | Fable | Threat modelling, auth/session/OIDC, SSRF, crypto use, supply chain | Agent tool: `subagent_type: expert-security` |
| `expert-webrtc-voice` | Fable | webrtc-rs SFU, dual PeerConnection, Opus/VP8, TURN/ICE, cpal audio | Agent tool: `subagent_type: expert-webrtc-voice` |
| `expert-rust-backend` | Opus | axum/tokio/sqlx/Valkey patterns, migrations, Kaiku module standards | Agent tool: `subagent_type: expert-rust-backend` |
| `android-push` | Gemini Argon | Android lifecycle/doze, FCM + UnifiedPush, Compose, Play Services | `scripts/agents/ask-gemini-expert.sh android-push "<question>"` |
| `ui-design-system` | Gemini Argon | Kaiku design tokens/contrast rules, Solid.js component consistency, i18n readiness | `scripts/agents/ask-gemini-expert.sh ui-design-system "<question>"` |

Gemini experts run read-only (`--mode plan`) with the repo as working
directory. Briefs live in `docs/developer-guide/agents/experts/`.
