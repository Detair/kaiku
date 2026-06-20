# Kaiku — Project Goals

> Status snapshot: 2026-06-09. Synthesized from `roadmap.md`, `tech-debt.md`,
> the beta checklist/stabilization plans, open issues/PRs, and a code-level
> verification pass. Complements `roadmap.md` (phase detail lives there);
> this document defines the *goals* and their ordering.

## Where the project stands

- **Phases 0–6 are ~98 % complete.** Beta is deployed and running on
  `kaiku.pmind.de`. Core platform (guilds, chat, threads, search, voice SFU,
  multi-stream screen share, E2EE DMs, bots/webhooks, moderation, GDPR
  governance, admin tooling) is implemented with 47+ integration test modules.
- **A native Android app exists** (`mobile/android`, Kotlin/Compose, WebRTC,
  QR login) — further along than the roadmap implies.
- **In flight:** dependency-update sweep (Phase 8/9 PRs #567/#568 in draft,
  #568 blocked on rusqlite #115).
- **Broken:** scheduled Security Audit workflow failing since 2026-05-17
  (4 consecutive runs).
- **Unstarted:** roadmap Phases 7 (SaaS polish), 8 (reliability), 10 (storage
  scaling). 14 open tech-debt items.

---

## Goal 1 — Restore and keep the project green (now)

The foundation for everything else: no red CI, no stalled PRs, no overdue
security checks.

- [x] Diagnose and fix the failing **Security Audit** workflow (failing since
      2026-05-17 — likely a new advisory or tooling drift).
      *Done 2026-06-10, PRs #571 + #572 — Security Audit run 27244132757 green
      across all 7 jobs. #571: lettre → 0.11.22 (RUSTSEC-2026-0141; Kaiku uses
      native-tls, not exploitable) + Windows CI fix (CMake 4.x runner image
      needed CMAKE_POLICY_VERSION_MINIMUM=3.5). #572: openssl → 0.10.80 (2026
      CVE batch), actix-http → 3.12.1 (lockfile-only, never compiled),
      mermaid → 11.15.0 (4 XSS CVEs), plus justified OSV ignores for
      audiopus_sys/proc-macro-error2 (unmaintained, no fix). Follow-up tech
      debt: migrate off audiopus_sys eventually.*
- [x] Land **PR #567** (webrtc-rs 0.11 → 0.17) — the largest remaining dep risk.
      *Done 2026-06-10 (`6b19f227`): rebased onto main preserving the #571/#572
      security pins, re-verified (948 server tests, deny, clippy), full CI green
      incl. all desktop builds. User waived the manual three-client canary in
      favor of CI evidence — first beta deploy after this should include a
      quick voice sanity check.*
- [x] Land **PR #568** (keyring 2 → 4) — merged 2026-06-10 (`3e5592a0`).
      *Rebased onto main; keyring 4 runtime-validated on Linux against the real
      Secret Service (set/get/delete round-trip mirroring auth.rs). User waived
      runtime tests on macOS/Windows/Android (CI compile evidence) — if session
      restore misbehaves there, it surfaces as re-login-required in beta.*
- [x] **sqlx 0.8.6 → 0.9 migration** + **rusqlite 0.32 → 0.38** (#115) —
      merged 2026-06-10 (`dacfaf2a`, PR #573); issue #115 auto-closed.
      *Migration surface: 6 QueryBuilder lifetime removals, SqlSafeStr
      adjustments (one AssertSqlSafe), .sqlx cache regenerated with
      `--all-targets` (56/79 entries updated by 0.9's nullability inference —
      plain prepare DROPS test-target queries, always pass `--all-targets`).
      948 server tests green incl. all #[sqlx::test]; rusqlite 0.38 runtime
      smoke passed.* **The dependency sweep (Phases 1–10) is now closed.**
- [x] **TD-03**: Q2 re-check done 2026-06-10 (`b328af26`, PR #574) — and
      *resolved* outright: aws-sdk-s3 1.132 requires `lru ^0.16.3`, so the
      vulnerable 0.12.5 left the tree; stale ignores removed from security.yml
      and .osv-scanner.toml. Bonus check the same day: RUSTSEC-2023-0071 (rsa
      Marvin, issue #291) still has no stable fix — rsa 0.10 is at rc.18;
      re-check when rsa 0.10 ships and jsonwebtoken/openidconnect adopt it.

**GOAL 1 COMPLETE (2026-06-10).** All scheduled workflows green; zero open
PRs; dependency sweep closed; every remaining advisory exception re-justified
with a dated rationale.

> **Keep-green maintenance 2026-06-20:** the scheduled Security Audit went red
> (OSV Scanner) after the 06-14 run — a mix of three stale `.osv-scanner.toml`
> ignores (instant/derivative/bincode, all dropped from the tree) plus newly
> disclosed npm advisories: dompurify (8 XSS), undici (7), vite (5 dev-server),
> @babel/core. Fixed by upgrading dompurify→3.4.11, vite→8.0.16,
> undici→7.28.0, @babel/core→7.29.6, removing the stale ignores, and
> re-justifying the test-only nested vite 7.3.1 dev-server CVEs (vitest JSX
> compat blocks its bump). OSV green again.

## Goal 2 — Close Phase 6: desktop client parity (next)

Five concrete items finish the phase (roadmap §Phase 6 remainder):

- [x] Desktop **connection metrics** (TD-16) — done 2026-06-10 (`dda79ec2`,
      PR #575). *Native RTT from RemoteInboundRTP receiver reports (NB:
      webrtc-rs reports it in ms, NOT W3C seconds — review caught a 1000×
      bug), receive-side loss + RFC 3550 jitter measured in the audio decode
      loop (webrtc-rs get_stats lacks both), shared quality thresholds in
      types.ts used by both adapters. Gotcha: CI's nightly rustfmt enforces
      group_imports (std first) that stable local rustfmt ignores.*
- [x] **Output device selection** — done 2026-06-10 (`3d3facfd`, PR #576).
      *The CPAL path was fine; the integration wasn't: stored setting never
      applied on join, mid-call switch was a no-op (now rebuilds the CPAL
      stream in place via PlaybackControl::SwitchDevice), settings page
      enumerated browser ids unusable by the native backend (now via the
      adapter), browser setSinkId targeted nonexistent DOM elements. User
      should verify mid-call switching on this Linux machine after next
      build.*
- [x] **WebSocket reconnect channel refresh** — done 2026-06-10 (`ec80299b`,
      PR #577). *The roadmap's suggested fix (call loadChannelsForGuild on
      reconnect) would have yanked users out of their current channel — that
      function auto-selects the first text channel. Added selection-preserving
      refreshChannelsForGuild() + DM list reload instead, parallel and
      failure-tolerant in reconnect recovery.*
- [x] **System tray** — done 2026-06-11 (`11d4e1d1`, PR #578). *Tauri 2 core
      tray (tray-icon feature, no plugin): Show/Quit menu, left-click restore,
      close-to-tray (label-guarded so pop-outs close normally), unread badge
      fed by a reactive guild+DM sum via tray_set_unread. Gotcha: run
      `cargo +nightly fmt` before pushing — CI enforces nightly-only options.*
- [x] **Auto-update** — done 2026-06-12 (`678e1a97`, PR #579). *Signed GitHub
      Releases channel: updater pubkey in tauri.conf.json, latest.json
      manifest assembled in release.yml, check-on-startup + Restart-now toast.
      Setup gotchas: GitHub cannot store EMPTY secrets — a passwordless key
      means NO password secret (workflows fall back via `|| ''`); every
      `tauri build` step needs TAURI_SIGNING_PRIVATE_KEY once a pubkey is
      configured. Private key: ~/.tauri/kaiku.key — USER MUST BACK IT UP
      (losing it permanently bricks auto-update for installed clients). E2E
      verification at the next release tag.*

**GOAL 2 COMPLETE (2026-06-12).** All five Phase 6 desktop items shipped
(PRs #575–#579); roadmap Phase 6 marked desktop-complete in PR #580.

## Goal 3 — Truthful docs: reconcile roadmap/tech-debt with reality

Code verification found drift that distorts planning:

- [x] **TD-29 (simulcast)** — rescoped 2026-06-12 (`de5d4c3a`, PR #580):
      server-side negotiation confirmed implemented; remaining gap is
      multi-layer publishing, blocked upstream on webrtc-rs. Same PR fixed
      stale TD-15 (webcam exists), TD-25 (Windows builds green), recorded
      TD-16 resolution, corrected the resolution summary (19/11), and checked
      off all Phase 6 desktop items in the roadmap.
- [x] Mobile milestone — done 2026-06-12 (`4722a049`, PR #581). *Full
      code-verified Android coverage audit: auth/WS/messaging/voice solid;
      5 beta blockers identified (markdown, attachments display, unread
      indicators, DM UI, reaction picker) with exit criteria in roadmap.
      iOS and store-distribution decisions remain Goal 7 gate questions.*
- [x] Tech-debt sweep — done 2026-06-11/12 (PR #580): all open items
      code-verified; TD-15/16/25 resolved, TD-29 rescoped; summary 19/11.

**GOAL 3 COMPLETE (2026-06-12).** Roadmap + tech-debt reflect verified code
state; mobile has an explicit milestone with exit criteria.

## Goal 4 — Beta operations hardening

From `2026-03-15-beta-launch-stabilization-plan.md` (partially overtaken by
the monitoring stack already running on the VPS — verify, then close):

- [x] Monitoring provisioning committed — verified 2026-06-12: already in
      repo (`infra/monitoring/` has otel-collector/prometheus/tempo/loki
      configs, grafana datasources + kaiku-overview dashboard, alert rules;
      compose has a `monitoring` profile). Live-VPS drift check requires
      operator (SSH inspection was permission-denied for the agent).
- [x] **Backup script + restore procedure** — done 2026-06-12 (`b4b35c68`,
      PR #582): backup.sh now also archives RustFS object data with gzip -t
      integrity checks; `docs/ops/backup-restore-runbook.md` documents
      restore for both stores + quarterly drill. **Found and fixed: the
      production compose never defined the rustfs service the VPS runs — a
      fresh VPS could not have been rebuilt from the repo.**
- [ ] **Operator actions (user, ~10 min on VPS):** run the one-time VPS
      verification checklist in `docs/ops/backup-restore-runbook.md`
      (backup cron present? recent backups? rustfs volume name? compose
      drift vs live canis-rustfs?) and perform + log the first restore
      drill. Follow-ups noted in the runbook: off-host backup sync,
      at-rest encryption.

**Done when:** a fresh VPS could be rebuilt from the repo alone (repo side
done); one restore drill performed and documented (user).

## Goal 5 — Phase 8 reliability gates (before any v1.0)

Reliability work protects self-hosters and is a prerequisite for calling the
platform 1.0, ahead of SaaS features. Priority order within the phase:

1. [x] **Tenancy & isolation verification** — COMPLETE 2026-06-12.
       PR #583 (`ba084637`): 10 HTTP-level tests (enumeration, cross-guild
       message CRUD with DB verification, search leak checks,
       leave-revokes-access, positive control). PR #584 (`ea9e723f`):
       realtime half — cross-tenant WS subscription denial, broadcast
       topic-scoping with positive control, typing gate; cache-key survey
       documented (all keys principal/resource-scoped by construction).
       *Learned: guild membership grants nothing — VIEW_CHANNEL comes from
       the @everyone role; owners bypass. Also: macOS DMG bundling
       (bundle_dmg.sh) is a known CI flake — rerun before diagnosing.*
2. [x] **Performance budgets as CI gates** — done 2026-06-12 (`09626d58`,
       PR #585). *Enforced: 3 bundle budgets (initial JS ≤300KB gz / CSS
       ≤25KB / largest chunk ≤170KB — the practical startup proxy, lazy
       chunks excluded) + 2 server latency budgets (message history <500ms,
       guild search <750ms; best-of-3, flake-resistant). Budgets needing a
       real app (true startup, voice join E2E, client memory) documented as
       manual in docs/developer-guide/development/performance-budgets.md
       with migration notes.*
3. [x] **Self-hosted upgrade safety** — done 2026-06-12 (`195c9cbc`,
       PR #586). *upgrade-preflight.sh (health, disk, backup <26h + intact,
       migration delta, compose validity, rollback digest pin) +
       docs/ops/upgrade-playbook.md (lockstep compatibility matrix, safe
       windows, two-path rollback). Verified: ZERO down-migrations exist —
       schema rollback is honestly documented as restore-from-backup.*
4. [x] **Operator supportability pack** — done 2026-06-12 (`de0c0444`,
       PR #587). *GET /api/admin/diagnostics (connectivity + pool pressure +
       disk + activity/error counts, telemetry fields degrade to null) and
       docs/ops/README.md runbook index with quick-triage order. Diagnostics
       bundle download deferred to the chaos/Phase-8 follow-up scope.*

**Goal 5's four priority items are COMPLETE (2026-06-12).** Remaining
scheduled Phase 8 scope: chaos drills, policy-as-code, FinOps, bot
hardening — plus the fold-ins below.
5. [ ] Chaos drills, policy-as-code, FinOps, bot hardening — schedule after
       the four above (still deferred — lower priority for a self-hosted beta).
6. [x] **Self-hosted STUN/TURN** — done 2026-06-12 (`0f4b3240`, PR #592).
       *Server defaulted STUN to Google even with self-hosted coturn (leak,
       present in .env.beta); added startup leak-warning + setup-beta STUN
       rewrite to the coturn endpoint + self-hosting docs. coturn + TURN HMAC
       creds already existed.*

Fold-ins: **TD-26** ✅ (`326004f3`, PR #588 — curated baseline wordlists,
substring-aware boundary-anchored regexes). **TD-27** ✅ resolved 2026-06-12
(`9c007546`, PR #589 — server-side session-id authority for voice stats,
pagination append in SessionList; the REST tests and finalize retry were
already present).

**GOAL 5 — four priority items + both production-blocking fold-ins COMPLETE
(2026-06-12).** Remaining scheduled Phase 8 scope (lower priority, deferred):
chaos drills, policy-as-code, FinOps, bot hardening.

## Goal 6 — Phase 7 user-facing maturity (selective)

Order by value to the current (self-hosted + beta) audience; billing only if
the SaaS path is actually pursued:

1. [~] **Accessibility (WCAG 2.1 AA)** — started 2026-06-12 (PR pending).
       Audit found the beta-checklist items (ServerRail `aria-current`,
       `role`/`aria-label`) were ALREADY done. Fixed 17 icon-only buttons
       across core chat UI that had only `title` (not announced by screen
       readers) → added `aria-label`. Added the contributor a11y checklist
       (`docs/developer-guide/development/accessibility.md`). *Remaining:
       full keyboard-only pass, NVDA/VoiceOver testing, automated gate
       (jsx-a11y is React-only / Solid-incompatible) — tracked in the doc.*
2. [x] **Admin command center** — verified already built 2026-06-12 (docs
       PR). 7 observability endpoints + 1031-line CommandCenterPanel wired
       into AdminDashboard with 30s polling + 30-day retention task. Roadmap
       reconciled (was stale "planned"); alert-authoring UI was a v1
       non-goal. Same PR also marked the Goal-5 Phase 8 items done in the
       roadmap (perf budgets #585, upgrade safety #586, tenancy #583/#584,
       operator pack #587).
3. [x] **SaaS observability** (Sentry/OTel client-side) — verified complete
       2026-06-12: client `lib/sentry.ts` (DSN-gated, PII + token stripping,
       wired in index.tsx), Tauri `sentry::init` (SENTRY_DSN_CLIENT-gated),
       server OTel (Goal 4). *Only remaining: release source-map upload —
       needs an external Sentry account (operator infra), so out of scope
       here.* [Original "partially present" framing was stale.]
4. [~] **Identity trust / OAuth linking** — medium priority. (OIDC login
       already exists; "linking" of multiple external identities to one
       account is the new surface.) **Foundation shipped 2026-06-20 (PR #600):**
       new `user_identities` table (authoritative `(provider_slug, subject)`
       login lookup, back-fills the legacy single `users.external_id`); OIDC
       login resolves and records identities through it. Verified: code review
       found the model was one-identity-per-account by construction; no linking
       existed. Remaining: (a) authenticated link/list/unlink endpoints +
       link-intent OIDC flow; (b) client "Linked Accounts" settings UI. Both
       build directly on this table.
5. [ ] **Billing & subscriptions (Stripe)** — defer until a SaaS launch
       decision is made; do not build speculatively.

## Goal 7 — Define and ship v1.0

**A concrete v1.0 definition now exists: `v1.0-definition.md`** (written
2026-06-12). It records what's already done, the buildable remainder
(Android beta, operator drill, release dry-run), and the three product
decisions that block v1.0 and need maintainer input (i18n, iOS, SaaS).

Original proposed gate (kept for reference):

- Goals 1–5 complete (green CI, Phase 6 closed, docs truthful, ops hardened,
  reliability gates 1–4).
- Goal 6 items 1–2 complete (a11y + admin command center).
- Android app in public beta with a documented feature matrix.
- Windows desktop build fixed or formally dropped for 1.0 (**TD-25**, libvpx).
- Decision recorded for: i18n (currently none — English-only), iOS, SaaS.

**Done when:** a `v1.0.0` release exists with signed desktop artifacts,
upgrade playbook, and a public changelog.

---

## Suggested sequencing

| Horizon | Goals | Theme |
|---------|-------|-------|
| Weeks 1–2 | G1, start G3 | Green CI, unblock deps, doc truth |
| Weeks 3–6 | G2, G4 | Finish Phase 6, ops hardening |
| Months 2–4 | G5, G6 (1–2) | Reliability gates, a11y, command center |
| Months 4–6 | G7 | v1.0 definition → release |

Out of scope until explicitly decided: federation, call recording, stickers/
polls, Phase 10 CDN scaling (SaaS-only), iOS client.
