# Task 2 report: per-device subscriptions and push config (C18/C19)

Status: DONE

## Implementation
- Migration `server/migrations/20261110000000_push_subscription_device.sql` (sorts after 20261023000000): device_id column, partial unique index (user_id, device_id), device index.
- `server/src/push/mod.rs`: `CreateSubscriptionRequest.device_id`; provider validation (unifiedpush unchanged, fcm -> `ProviderUnavailable` when FCM off, token regex equivalent check, else Validation; error never echoes the token); one-transaction device upsert in the brief's order, then prune to `MAX_SUBSCRIPTIONS_PER_USER` (10); legacy path (no device_id) keeps the `(user_id, endpoint)` upsert plus prune; `get_config` (C18), `delete_device` (204 always, own rows only), routes `/config` and `/device/{device_id}`; `PushSubscription.device_id` (also in the redacted Debug); `PushError::ProviderUnavailable` -> 400 `PUSH_PROVIDER_UNAVAILABLE`.
- Handlers carry no `utoipa::path` (only ToSchema), so new response types derive ToSchema; no OpenAPI path changes.
- CHANGELOG `### Changed` entry added.

## TDD
RED: 9 new tests in `push_http.rs` failed (`9 passed, 9 failed` on the push_http filter) before implementation. GREEN: 20/20 push tests (integration), 23/23 lib push tests.

## Gates
- Full `cargo nextest run -p vc-server`: 1216 passed, 33 skipped.
- `cargo +nightly fmt --check`: clean. clippy -D warnings: clean. `cargo deny check licenses advisories`: ok. `check_ci_guardrails.py`: passed.

## Concerns
- Prune never deletes the row just written (`id <> $just`). The brief's order key alone could prune a re-registered old row (created_at unchanged by the upsert); this deviation is deliberate and safe.
- Pre-existing uncommitted modifications to `client/src-tauri/gen/schemas/*` are not mine and were not committed.
- No `.sqlx` changes (runtime queries only).

## Fix round 1
- P8: removed the cross-user delete by device_id (client-asserted); cross-user cleanup only on exact (provider, endpoint), now also in the legacy path. AGENTS.md notes proof of possession.
- endpoint validator max 4096; upsert keeps last_used_at; per-user `pg_advisory_xact_lock(hashtextextended(user_id,0))` at transaction start (both paths); prune still excludes the written row.
- Migration: added (provider, endpoint) index, dropped the device_id index (delete-by-device is covered by the unique (user_id, device_id) index).
- fcm.rs access_token(): after a failed fetch the cached token's expiry is re-checked.
- Tests: replaced test_device_id_moves_to_new_user with foreign-device-id-keeps-row and same-endpoint-moves; added legacy move, same-user new-device, legacy-row adoption, 4096-char token.
- Gates: push tests 49/49, full nextest 1222 passed, fmt, clippy, deny, guardrails clean.
