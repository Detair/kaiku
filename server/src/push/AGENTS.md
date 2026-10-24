<!-- Parent: ../../AGENTS.md -->

# Push Module

Content-free mobile wake signals (contract C20). A push carries only `t`,
`channel_id` and `count`; the app fetches content over its normal
authenticated channel.

## Files

- `mod.rs` - `WakeSignal`, `deliver` / `spawn_dispatch`, `send_one` (provider
  dispatch), UnifiedPush transport, subscription CRUD.
- `fcm.rs` - FCM HTTP v1 transport: service-account loading, RS256 OAuth2
  assertion, cached access token (single flight), `fcm_message` (C20),
  `classify_fcm_response`.
- `service.rs` - `PushService` on `AppState.push`; holds the FCM client when a
  service account is configured.

## Transports

- `unifiedpush`: the endpoint is user-supplied. It must stay https-only and go
  through `webhooks::ssrf` (`verify_resolved_ip` + `pinned_client`) before
  every send.
- `fcm`: fixed hosts (`oauth2.googleapis.com`, `fcm.googleapis.com`); the
  token URI is validated at load time. FCM is off without
  `FCM_SERVICE_ACCOUNT_FILE` / `FCM_SERVICE_ACCOUNT_JSON`; an `fcm` row is then
  kept and skipped (`Delivery::Failed`).
- Pruning: only a definitely dead token is `Gone`: FCM 404 together with
  errorCode `UNREGISTERED`, or 403 together with `SENDER_ID_MISMATCH`
  (UnifiedPush: 404/410). A bare 404 (wrong URL or project) and any 400 (even
  one carrying `UNREGISTERED`) keep the row, or every device would lose push.
- A failed OAuth2 token fetch puts the client in a 30 s cooldown: sends fail
  fast without contacting Google, so an outage does not queue behind the
  single-flight lock.
- The FCM `reqwest` client honours `HTTPS_PROXY` / `HTTP_PROXY` (reqwest
  default). This is intentional, so deployments behind an egress proxy work.

## Secrets rule

The FCM private key, OAuth2 access tokens, FCM registration tokens and
UnifiedPush endpoints never appear in logs, error messages, `Debug` output,
URLs or the repo. FCM logs carry only the HTTP status and the FCM `errorCode`.
`ServiceAccountKey`, `FcmClient` and `PushSubscription` have hand-written
`Debug` impls.

## Tests

- Unit tests in `fcm.rs` use a local axum mock on 127.0.0.1.
- Integration tests (`tests/integration/push_http.rs`) use `spawn_fcm_mock` and
  `test_service_account` from `tests/integration/helpers`. The RSA key in
  `server/tests/fixtures/fcm_test_key.rs` is test-only. No test may reach the
  internet.

## Registration trust

Cross-user cleanup on registration matches only an exact (provider, endpoint):
possessing the token proves the phone changed hands. A `device_id` is
client-asserted, so it never deletes another user's rows.

Endpoints (FCM tokens included) are capped at 2048 bytes: they sit in btree
indexes, whose entries are limited to about 2.7 KB. Registration takes two
advisory locks in a fixed order: the per-user lock (seed 0), then the
(provider, endpoint) lock (seed 65, not in the `db/mod.rs` registry).

## Single replica

The access-token cache is per process. Any per-user push state (e.g.
coalescing) kept in memory assumes a single server replica; running several
replicas needs shared state first.
