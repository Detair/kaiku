# Goal: Kaiku v1.0 "Family Release" by Christmas 2026

> Written 2026-10-20/22 with the maintainer. Supersedes the audience framing of
> `docs/developer-guide/project/v1.0-definition.md` (2026-06-12/07-04) — the
> scope decisions there (self-hosted, English-only, Android-only, no SaaS)
> still hold unless changed below; this document sharpens *who* v1.0 is for,
> records the build-vs-adopt decision, cuts scope, and defines the review gate.

## Intent

Get the maintainer's family and friends using Kaiku as their everyday
communication tool on the maintainer's own server (`kaiku.pmind.de`) by
**Christmas 2026** (~9 weeks from writing). They should be able to drop
WhatsApp/Discord for the use cases below without noticing a downgrade.

The project paused after 2026-07-14 with the v1.0 gate nearly met, but the
last burst of work (forum/announcement channels, scheduled events, reaction
roles, embeds/components, webhooks, membership screening, push foundation)
went into Discord-parity features that were not part of the gate. This goal
re-centres on shipping.

## Who and how (decided by the maintainer)

- **Audience:** family and friends on the maintainer's server. Opening Kaiku
  to the public / third-party self-hosters is a **later** release.
- **Usage pattern:** **evenings on PC with voice** (gaming), **daytime chat on
  the phone**.
- **Devices:** everyone is on **Android**. No iOS users → iOS stays post-1.0.
- **Key UX criterion:** **one app for everything** that non-technical people
  can use without help. The maintainer can juggle several apps; the family
  cannot.
- **v2 (Matrix hybrid rewrite, `kaiku-v2/`) is off the table.**

## Decision record: build (Kaiku) vs. adopt (2026-10-22)

The maintainer questioned whether building Kaiku is better than using an
existing solution. Three alternatives were researched against the family
criteria (Android install for non-techies, push without friction, Discord-style
persistent voice, self-hosting effort, project health):

| Option | Outcome |
|---|---|
| **Mumble + separate chat** | Rejected: two apps, no chat history/offline messages/push in Mumble — violates "one app". |
| **Stoat (ex-Revolt)** | Rejected: official Android app doesn't support self-hosted instances; push requires own Firebase project + own app build; Android voice unverified; ~10-service compose stack with data-loss-prone manual migrations. |
| **Matrix + Element X / Element Call** | Rejected for this use case: excellent phone chat + push, but no persistent voice channels or push-to-talk (call-in-room model), desktop gamers would need a different client (Cinny/Rabble) than phones; ESS is k8s-only with monthly breaking changes. |
| **Kaiku** | **Chosen**: the only option with one app covering both Discord-style evening voice and daytime chat. Desktop voice is near-ready; the gap is Android (see below), estimated 15–25 focused days (agent implements, maintainer reviews). |

Accepted trade-off: single-maintainer burden and custom WebRTC stack
(webrtc-rs is largely in maintenance mode upstream). Re-evaluate post-1.0.

## Scope for v1.0 (family scope)

**In — family-critical (from the 2026-10-22 code audit, verified):**

1. **Android push notifications.** **FCM is the default transport**,
   **UnifiedPush optional** (server already supports it). Push for DMs **and
   @mentions** (server currently pushes DMs only). Payload stays content-free
   (wake signal; app fetches from the server). Needs: Firebase project
   (maintainer), FCM sender in `server/src/push`, Android FCM + UnifiedPush
   connector, notification channels, wake-fetch worker, deep link into
   channel/DM.
2. **Android realtime reliability.** Fix the dead-end where an expired access
   token puts the WebSocket into `TokenExpired` with `shouldReconnect=false`
   and nothing refreshes/reconnects (`KaikuWebSocket.kt` ~143–147, 215–222) —
   after doze or >15 min offline, live updates stop until app restart. Add
   catch-up of missed messages and a reconnecting indicator.
3. **E2EE off for v1.0, DMs must always work.** Today a desktop user with E2EE
   set up cannot DM an Android-only user at all (`sendEncryptedDM` throws "No
   recipients" and never falls back, `client/src/stores/messages.ts` ~571–601),
   and Android renders encrypted DMs as raw JSON. For v1.0: E2EE setup is not
   offered by default; a plaintext path always works; Android shows a clear
   placeholder for any encrypted message. Real Android E2EE (vodozemac + key
   backup) is post-1.0.
4. **Installable Android app.** Signed release APK built by `release.yml`,
   proper versioning; distribution by sideloaded APK from the maintainer
   (store/F-Droid post-1.0, unless the open question below changes it).
5. **Onboarding without friction.** Accounts created by the maintainer (or
   invite flow that works on Android), login via QR from desktop or a simple
   URL; first-run path tested with a non-technical mindset.
6. **Daily-chat essentials on Android** as far as they matter: mentions,
   attachment upload, reply — prioritised after 1–5.
7. **Release mechanics** from `v1.0-definition.md`: restore drill, `v1.0.0-rc`
   dry-run (signed desktop + updater manifest + APK), updater key backed up
   off-host, #623 fixed, curated changelog.

**Out / frozen:**
- No new features. July features stay as-is; the review may recommend hiding
  one behind a flag for v1.0 instead of fixing it (maintainer decides).
- Public/community release, iOS, i18n (unless open question changes it),
  SaaS/billing, federation, Android E2EE, Phase 10 scaling.

## Anti-goals (would make this a failure even if it "works")

- Family members miss messages because the phone stays silent.
- A security incident on the internet-exposed server (account takeover,
  data leak, SSRF into the home network/VPS).
- Losing data with no working restore.
- UI that confuses non-technical users so they give up in the first week.
- Spending the 9 weeks polishing findings that don't matter for this audience.

## Constraints

- Project rules in `CLAUDE.md` apply (feature branches/worktrees + squash-merge
  PRs, `cargo deny` license constraints — Firebase Android SDK is Apache-2.0,
  check any server-side FCM crate, UI contrast rules, changelog, quality gates).
- Live VPS/beta actions (deploys, drills, Firebase project creation) are the
  maintainer's; review work is **read-only** and does not log into the live
  beta.

## Sequencing

1. **Review phase (now, read-only, parallel lanes below)** — produces the
   blocker list for the family scope; runs alongside planning.
2. **Implementation plan** (superpowers:writing-plans) from scope items 1–7 +
   review blockers, ordered: push → reconnect → DM/E2EE fallback → APK/release
   → onboarding → chat essentials.
3. **Household week** on `v1.0.0-rc` (maintainer's household).
4. Tag **`v1.0.0`**, then **family rollout before Christmas**.

Cut order if time runs short: chat essentials (6) → UnifiedPush option →
mention push (keep DM push) — never push itself, reconnect, or DMs working.

## Threat model for the security review

Internet-exposed single server, trusted user base. Priority order:
1. **Unauthenticated external attacker:** registration/invite gating, login,
   OIDC, password reset, tokens/sessions, rate limiting, exposed
   admin/diagnostics/metrics endpoints, CORS, file upload/serving, incoming
   webhook token secrecy, push/webhook SSRF guard, WebSocket auth + flood
   limits, TURN credentials, reverse-proxy/TLS, secrets in repo/images/CI.
2. **Compromised single account** (stolen phone, leaked token): IDOR/tenancy,
   privilege escalation via roles/reaction roles/bots, session revocation,
   Android token storage.
3. **Supply chain:** cargo audit/deny, OSV, bun audit, Android deps, signing
   keys (Tauri updater key, Android keystore, Firebase credentials).
Insider abuse among family is low priority.

## Review plan

Six specialized lanes, run in parallel, read-only, against `main` @
`aa1f319f`. Each lane runs twice independently: **Fable 5.1** (primary) and
**Gemini 4 Argon** (external); Opus consolidates and re-verifies Blockers.
Reviewers may consult the experts. Each finding: severity (**Blocker** for family v1.0 / **Before
rollout** / **Post-1.0**), file:line evidence, concrete failure scenario, fix
sketch. Unverified suspicions labelled as such. Every Blocker is
re-verified against the code before it is reported.

| # | Lane | Scope | Focus |
|---|------|-------|-------|
| 1 | **Security** | server, infra (compose, Caddy, workflows), client token storage (web, Tauri, Android) | Threat model above; extra scrutiny on code since 2026-07-01 (webhooks, push, components, screening, events, crosspost) |
| 2 | **Implementation — server & core flows** | server correctness, error handling, migrations, concurrency, tests | July features under real use; core loop (send/edit/unread/reconnect/voice join); silent failures; #623; what the push/mention work will touch |
| 3 | **Implementation — desktop client** | `client/src`, `client/src-tauri` | Voice robustness (dual PC, mixer, device switch, TURN), reconnect, auto-update, tray, session restore; E2EE prompt + DM fallback path |
| 4 | **Android readiness** | `mobile/android` | Beyond the known gaps above: lifecycle/doze, token refresh, voice on Android, crash risks, test state, what FCM/UnifiedPush integration needs architecturally |
| 5 | **UI consistency** | web/desktop UI + Android visual parity | Design tokens & contrast rules, component reuse vs. one-offs (July UIs vs. established patterns), empty/error/loading states, wording, a11y basics, first-run flow for a non-technical user (code-based) |
| 6 | **Release & ops readiness** | `docs/ops`, `scripts`, `release.yml`, `tauri-build.yml`, Android build | Can `v1.0.0-rc` ship: updater pipeline, APK signing in CI, backup/restore, #623, monitoring that reaches the maintainer; Windows SmartScreen/macOS Gatekeeper impact |

## Deliverables

1. `docs/reviews/2026-10-22-v1-family-release-readiness.md` — consolidated
   findings by lane and severity.
2. Updated `v1.0-definition.md` (family audience, revised gate), `roadmap.md`
   and `goals.md` (Goal 8: Family Release; post-1.0 track: Android E2EE,
   community/public release, store distribution, iOS, i18n, SaaS; re-evaluate
   voice stack dependency risk).
3. Recommendation: ordered work packages to Christmas with sizing and cut
   order (in the review doc, summarised in chat).

## Left to the builder

- Agent types/prompts per lane and how to split large lanes.
- Running read-only tooling (audits, clippy, tests) during review.
- Format of the consolidated review doc; FCM server crate choice (license-checked).

## Answers & decisions (2026-10-22)

- **Who builds:** agents build and review; the maintainer steers. Kaiku
  product/code decisions are delegated to the agents; **infrastructure
  changes (VPS, DNS, Firebase project, signing certificates, CI secrets)
  must be cleared with the maintainer first.**
- **Multi-model setup:** Opus 5.5 builds, Fable 5.1 reviews, and **every
  review additionally gets at least one external model** (Gemini 4 Argon via
  `agy`; Codex once installed). Domain experts are consultable by all agents.
  See `docs/developer-guide/agents/multi-model-workflow.md`.
- **Family PCs: Windows, Linux and macOS** → all three desktop builds are
  in scope. Unsigned Windows/macOS installers trigger SmartScreen/Gatekeeper
  warnings; whether to buy signing (Apple Developer ID ~99 USD/yr, Windows
  code-signing cert) or ship with install instructions is an
  infrastructure/cost decision for the maintainer (lane 6 sizes both).
- **No real data on the beta** — test data only. Upgrades may reset the
  beta; the restore drill is still required before the family rollout, but
  is no longer about protecting existing users.
- **i18n: English for v1.0, German likely later — design for it now.** v1.0
  must be *i18n-ready*: a string-catalog layer in the web/desktop client that
  all new and touched UI uses, Android strings in `strings.xml`, and no new
  hardcoded user-facing strings. Full extraction/translation is post-1.0.
  Lane 5 measures the current hardcoded-string surface.
