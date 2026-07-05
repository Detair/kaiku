# Identity-Linking Review Follow-ups — Implementation Plan

> Created 2026-06-20 from a full code review (coding patterns, docs accuracy,
> security) of the OAuth identity-linking feature: PR #600 (`user_identities`
> table + login lookup refactor), #601 (link/list/unlink endpoints + OIDC link
> flow), #603 (client view+unlink UI). All findings are tracked as GitHub
> issues #604–#612. This plan groups them into ordered work phases.

## Context

The review found the feature **fundamentally sound** — no critical/high
security issues, conventions broadly followed, OpenAPI/CHANGELOG/goals.md
accurate. The follow-ups are one medium security hardening item, a backfill
edge case to verify against production, reference-doc drift, and minor pattern
cleanups. None block the merged work; they harden and tidy it.

## Findings → issues

| # | Issue | Severity | Dimension |
|---|-------|----------|-----------|
| F1 | [#604](https://github.com/Detair/kaiku/issues/604) Rate-limit `link_authorize` | Medium | security |
| F2 | [#605](https://github.com/Detair/kaiku/issues/605) Link callback postMessage on error | Low | security/UX |
| F3 | [#606](https://github.com/Detair/kaiku/issues/606) Reconcile colon-less `external_id` | Low | security/data |
| F4 | [#607](https://github.com/Detair/kaiku/issues/607) Update reference docs / AGENTS.md | Medium | docs |
| F5 | [#608](https://github.com/Detair/kaiku/issues/608) Tech-debt entries + stale comment | Medium | docs |
| F6 | [#609](https://github.com/Detair/kaiku/issues/609) roadmap SSO overstatement | Low | docs |
| F7 | [#610](https://github.com/Detair/kaiku/issues/610) `db_error!` on new queries | Low | pattern |
| F8 | [#611](https://github.com/Detair/kaiku/issues/611) Remove unused `count_user_identities` | Low | pattern |
| F9 | [#612](https://github.com/Detair/kaiku/issues/612) Extract shared `formatRelativeTime` | Low | pattern |

## Phase 1 — Security hardening (do first)

**PR A: rate-limit the protected auth routes (F1, #604).**
- In `server/src/auth/mod.rs`, wrap `link_authorize` (and ideally the whole
  `protected_routes` block) with `rate_limit_by_user` + `RateLimitCategory::AuthOther`,
  mirroring the public `oidc_authorize_route` (`:103-111`). `link_authorize` is
  the priority (external OIDC amplification); `list`/`unlink`/MFA-setup are
  defense-in-depth.
- Add an integration test asserting repeated `link_authorize` calls get a 429
  after the category limit (see `ratelimit_http.rs` for the harness).
- Verify: `cargo test`, `clippy -D warnings`, nightly fmt.

**Operator action (F3, #606)** — independent, no code:
- Run against production: `SELECT id, external_id FROM users WHERE
  auth_method='oidc' AND external_id IS NOT NULL AND position(':' IN external_id)=0;`
- If zero rows → close #606 (impact nil, the `{slug}:{subject}` format held).
- If rows exist → reconcile (insert correct `user_identities` rows) before
  relying further on the `user_identities` lookup; optionally add a startup warn.

## Phase 2 — Documentation accuracy (cheap, high-signal)

**PR B: reference-doc refresh (F4 #607, F5 #608, F6 #609).** A single docs PR:
- `architecture/overview.md` §5 + endpoint list: add `user_identities` as the
  authoritative OIDC lookup; add the three `/auth/me/identities*` endpoints.
- `server/src/auth/AGENTS.md`: rewrite the "User Linking" note (now `user_identities`
  + link/unlink + error codes), add `identities.rs` to Key Files.
- `server/src/db/AGENTS.md`: add `user_identities` to the tables list.
- `server/migrations/AGENTS.md`: add the migration row or de-hardcode the list.
- `tech-debt.md`: add the two follow-ups (external_id UNIQUE demotion;
  deferred link-add UI). Reword the future-tense `login.rs:660-664` comment to
  present tense and point it at the new tech-debt item.
- `roadmap.md:317`: soften the PR #135 "automatic account linking" claim; add a
  2026-06 identity-linking entry.
- Verify: docs-only, "Docs Governance" CI.

## Phase 3 — Pattern cleanups (batch into one small PR)

**PR C: convention tidy (F7 #610, F8 #611, F9 #612).**
- Add `db_error!` to `insert_user_identity` (and optionally the
  `unlink_identity_guarded` statements / `subject` field on `find_user_id_by_identity`).
- Remove `count_user_identities`; switch the two test assertions to
  `list_user_identities(...).len()`.
- Extract `formatRelativeTime` to `client/src/lib/` and import in both
  `SessionsSection` and `LinkedAccountsSection` (settle on one casing).
- Verify: `cargo test` + `clippy` (server), `bun run test:run` + `bun run build`
  (client).

## Phase 4 — Link-add feature (the remaining product slice, larger)

Not from the review — this is the deferred Goal 6 item 4 work, but F2 (#605)
belongs here:
- Native Tauri command `oidc_link_identity` (mirror `oidc_authorize`, but send
  the user's `Bearer` token to `/auth/me/identities/authorize/{provider}` and
  expect a `?linked=<slug>` callback).
- Server: make `handle_identity_link` redirect link **errors** (not just
  success) to the localhost/SPA callback so the popup always closes (F2, #605).
- Client: "Link account" buttons in `LinkedAccountsSection` + an
  `oidc-link-callback` postMessage receiver; gate browser-mode appropriately.
- This PR carries the user-facing CHANGELOG entry for *adding* linked accounts.

## Suggested sequencing

1. **Phase 1 PR A** (security, #604) — highest value, small.
2. **Operator runs the #606 query** (parallel, no code).
3. **Phase 2 PR B** (docs) and **Phase 3 PR C** (patterns) — independent, can land in either order.
4. **Phase 4** (link-add) — its own larger effort; closes #605 and finishes Goal 6 item 4.
