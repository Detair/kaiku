<!-- Parent: ../../AGENTS.md -->

# Push Module

Content-free mobile wake signals (contract C20). A push carries only `t`,
`channel_id` and `count`; the app fetches content over its normal
authenticated channel.

## Files

- `mod.rs` - `WakeSignal`, `deliver`, `send_one` (provider dispatch),
  UnifiedPush transport, subscription CRUD.
- `fcm.rs` - FCM HTTP v1 transport: service-account loading, RS256 OAuth2
  assertion, cached access token (single flight), `fcm_message` (C20),
  `classify_fcm_response`.
- `service.rs` - `PushService` on `AppState.push`; holds the FCM client when a
  service account is configured and the `Coalescer`. `notify` (coalesced
  wake) and `notify_read` (read gate) are the only entry points for triggers.
- `coalesce.rs` - `Coalescer`: per-(user, channel) window, per-user cap, read
  gate, idle sweep. Pure `Instant` arithmetic; unit-tested.
- `targets.rs` - `wake_targets` (DM participants; guild `@mention` targets via
  `chat::mentions`) and `spawn_wake_for_new_message`, called from
  `chat::send::dispatch_new_message` for text and file messages.

## Triggers and coalescing (C20)

- Sending never waits on push: `spawn_wake_for_new_message` resolves targets
  (mention resolution costs a channel-access check per candidate) inside a
  spawned task.
- Per (recipient, channel): the first message pushes at once (`count` 1);
  messages within `WINDOW` (10 s) fold into one trailing push with the folded
  count; a non-empty flush re-arms the window. At most
  `USER_MAX_PER_MINUTE` (30) pushes per user per sliding minute, trailing
  flushes included; over the cap a wake is dropped.
- `read` goes out only when a dm/mention wake for that (user, channel) was
  sent within `READ_GATE` (24 h) and not cleared yet, and only for channels a
  mark-read handler actually marked (`chat::channels::mark_as_read`,
  `chat::dm::mark_as_read` / `mark_all_dms_read`, `chat::unread::mark_all_read`,
  `guild::settings::mark_all_channels_read`). Consuming the read gate has no
  other effect: the window and a pending trailing push stay, so a message that
  arrives after the read cursor (or between the read query and `notify_read`)
  is still pushed, and alternating message/read does not produce repeated
  immediate wakes.
- A flush timer that has not run a full `WINDOW` after its deadline counts as
  lost; the next message pushes immediately instead of waiting forever.
- Ordering on the device is not guaranteed: FCM does not order messages, and
  target resolution and delivery are asynchronous, so a `read` can arrive
  before or after a wake for the same channel. `read` and wakes share
  `collapse_key` = channel id, so FCM may drop a `read` while the phone is
  offline. Clients must therefore, on every wake, fetch the messages after the
  server's read cursor; if none are unread, they must clear any existing
  notification for that channel (the `read` may have been collapsed or
  reordered). They should also clear the channel's notification on `read`.
- `/api/me/read-all` and guild read-all mark only text channels, so a mention
  wake in an announcement or forum channel is cleared only by a per-channel
  read.
- Concurrency: the coalescer state is a `std::sync::Mutex` that is never held
  across `.await`; timers are spawned `sleep_until` tasks that call `flush`.
- Memory: every 256th `admit` sweeps entries idle for 1 h that hold no
  unexpired read gate (gated entries live at most 24 h). Rate history per user
  is dropped once older than a minute. A cap-dropped admission allocates
  nothing (the cap is checked before an entry is created).
- `MAX_ENTRIES` (65,536) is a hard cap. Trade-off: at the cap a new entry
  evicts every entry idle for 1 h, **read gates included**, so those channels
  get no `read` signal (their phone notification stays until opened or
  replaced). If nothing is idle, the message is pushed immediately without
  coalescing and without state (no read gate); eviction is retried at most
  once per second.

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
client-asserted, so it never deletes another user's rows. A UnifiedPush endpoint
on a guessable topic (e.g. ntfy) is a weaker possession proof than an FCM token;
registering the same URL from another account evicts the first account's row
(C19 mandates this).

Endpoints (FCM tokens included) are capped at 2048 bytes: they sit in btree
indexes, whose entries are limited to about 2.7 KB. Registration takes two
advisory locks in a fixed order: the per-user lock (seed 65,
`push_register_user`), then the (provider, endpoint) lock (seed 67,
`push_register_endpoint`); both are in the `db/mod.rs` registry.

## Single replica

The access-token cache and the coalescer (windows, per-user cap, read gates)
are per process and assume a single server replica. Several replicas would
each coalesce and rate-limit on their own (more pushes, read gates that miss);
that needs shared state (e.g. Valkey) first. A restart forgets pending
trailing pushes and read gates.
