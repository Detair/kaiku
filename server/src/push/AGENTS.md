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
- Pruning: only a definitely dead token is `Gone` (FCM 404 / `UNREGISTERED`,
  403 `SENDER_ID_MISMATCH`; UnifiedPush 404/410). A 400 means a bug in our
  payload and must never prune, or every device would lose push.

## Secrets rule

The FCM private key, OAuth2 access tokens, FCM registration tokens and
UnifiedPush endpoints never appear in logs, error messages, `Debug` output,
URLs or the repo. FCM logs carry only the HTTP status and the FCM `errorCode`.
`ServiceAccountKey` and `FcmClient` have hand-written `Debug` impls.

## Tests

- Unit tests in `fcm.rs` use a local axum mock on 127.0.0.1.
- Integration tests (`tests/integration/push_http.rs`) use `spawn_fcm_mock` and
  `test_service_account` from `tests/integration/helpers`. The RSA key in
  `server/tests/fixtures/fcm_test_key.rs` is test-only. No test may reach the
  internet.

## Single replica

The access-token cache is per process. Any per-user push state (e.g.
coalescing) kept in memory assumes a single server replica; running several
replicas needs shared state first.
