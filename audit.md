# Kaiku Project Audit

Date: 2026-02-14
Scope reviewed: `docs/project/roadmap.md`, `CHANGELOG.md`, `docs/plans/*.md`

## What this project is about

Kaiku is a self-hosted voice/text collaboration platform for gaming communities, built to evolve from a prototype into a production-grade, multi-tenant SaaS-capable system.

From the roadmap, changelog, and plans, the core strategic direction is:
- deliver Discord-like core collaboration (guilds, channels, DMs, voice/video),
- add strong safety/security and admin governance,
- optimize performance and reliability for daily use at scale,
- then expand into ecosystem capabilities (bots/plugins/webhooks), mobile support, and SaaS operations.

## Feature Audit Findings

## 1) Core platform progress (strong)

### Delivered/very mature
- Guild/server architecture, DM/group DM, navigation rails, and unified home UX are documented as complete.
- Permission model and rate limiting are implemented and repeatedly hardened in follow-up work.
- Realtime foundations (WebSocket/Redis sync patterns) are present across messaging, read state, and voice events.
- Security and correctness hardening is active (race condition fixes, auth boundary fixes, access control checks, MIME/content validation).

### Assessment
The project has moved beyond prototype basics. Core chat/voice infrastructure exists and is actively hardened rather than merely extended.

## 2) Voice and media stack (advanced, but mixed maturity in roadmap/plans)

### Delivered/very mature
- DM voice calls and substantial screen-sharing functionality are repeatedly represented as implemented.
- Rich voice diagnostics/connectivity monitoring appears implemented.
- Multi-track handling and Tauri/browser parity have progressed significantly.

### Partial/in-progress
- Multi-stream support still has open sub-work in roadmap/plans.
- Some planning artifacts suggest stale checklist states despite later completion claims.

### Assessment
Voice/media is a differentiator and already quite deep, but documentation state is inconsistent and creates uncertainty about exact completion boundaries.

## 3) Security, trust, and safety (good momentum, needs operational formalization)

### Delivered/very mature
- Auth improvements: OIDC/SSO, forgot password/reset, first-user setup model, JWT hardening, MFA-related work.
- Safety/moderation features: user blocking/reporting/admin workflows are in active implementation streams.
- E2EE direction: key backup foundation and E2EE message workflows are planned and partially/fully implemented in parts.

### Partial/in-progress
- Moderation has iterative v1/v2 documents with remaining checklist uncertainty.
- Some roadmap notes still flag security-adjacent gaps (e.g., channel-level permission filtering in parts of search scope).

### Assessment
Security posture is improving rapidly at feature level, but still needs stronger system-level verification and ongoing operational controls.

## 4) UX and productivity features (very active and user-facing)

### Delivered/very mature
- Command palette, modernized interaction patterns, context menus, unread aggregation, favorites, emoji/mention/spoiler improvements, synced preferences.
- Search/discovery has major investment and appears broadly available.

### Partial/in-progress
- Some "production polish" and "friction reduction" work exists as design docs without full implementation closure.

### Assessment
The product is already shifting from "feature parity" to "quality-of-life" and workflow acceleration, which aligns well with long-term retention goals.

## 5) Ecosystem, mobile, and SaaS readiness (planned more than shipped)

### Planned/early
- Mobile support has a clear design direction (Android-first plan), but appears mostly pre-implementation.
- Plugin/WASM, webhooks, advanced bot ecosystem, monetization, and compliance/governance are roadmap-heavy and plan-light in implementation detail.

### Assessment
This area is strategically correct but not yet execution-ready across all critical operational details.

## Cross-Document Consistency Issues

1. **Roadmap status ambiguity**: "current phase in progress" and "100% complete" signals appear simultaneously in places.
2. **Superseded plan sprawl**: several docs are replaced by v2/newer variants but older docs remain discoverable without strong archival markers.
3. **Completion confidence gaps**: some features are "complete" in roadmap/changelog while checklist-level plan docs still show open items.
4. **Unreleased changelog breadth**: large amount of work grouped together reduces milestone clarity for external readers.

## Recommended Additions (to better achieve your goals)

These are the highest-leverage additions based on your stated trajectory (production-grade + scalable + secure + self-hosted/SaaS capable):

## A) Create an SRE/Operations foundation track (high priority)

Add a dedicated plan set for:
- explicit SLOs (voice latency, reconnect success, message delivery freshness, API p95),
- observability standards (logs/metrics/traces dashboards + alert thresholds),
- incident response playbooks and postmortem template,
- load/capacity test gates tied to releases.

Why: many feature-level optimizations exist, but operational excellence is not yet a first-class, auditable program.

## B) Add backup/restore + disaster recovery drills (high priority)

Define and regularly test:
- PostgreSQL + object storage + key material backup policies,
- encrypted backup lifecycle and retention windows,
- recovery-time and recovery-point objectives,
- automated restore verification in staging.

Why: this is mandatory for credible self-hosted and SaaS trust.

## C) Introduce release governance (high priority)

Implement:
- feature flags with kill switches,
- staged rollouts/canaries,
- rollback procedures for server/client schema coupling,
- release notes split per milestone (not one large Unreleased block).

Why: current delivery velocity is high; release safety and reversibility must match it.

## D) Consolidate documentation source-of-truth (medium priority)

Add lightweight governance:
- "active/superseded/archived" front-matter for every plan,
- one feature inventory matrix (feature -> owner -> status -> canonical doc -> last validated date),
- monthly roadmap/changelog reconciliation check.

Why: this reduces planning drift and makes audits faster and more reliable.

## E) Formalize security verification cadence (medium priority)

Add recurring controls:
- threat-model refresh for auth/voice/media paths,
- permission-boundary regression suite as a quality gate,
- dependency/license/security scan reporting in release artifacts,
- annual external penetration testing plan.

Why: many good fixes are shipping; formal verification turns reactive hardening into predictable assurance.

## Suggested next planning focus (90-day practical sequence)

1. SRE/observability + SLO baseline
2. Backup/restore + DR rehearsal
3. Release governance/feature flags/canary rollout
4. Documentation consolidation matrix
5. Security verification cadence institutionalization

This sequence maximizes production confidence without slowing feature development.

## Final conclusion

Your project direction is strong and unusually ambitious for a self-hosted communication platform. The main opportunity is no longer "more features"; it is improving operational certainty and execution clarity so your existing feature depth translates into dependable production outcomes.
