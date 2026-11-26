<!-- Parent: ../../AGENTS.md -->

# Push Module

Content-free mobile wake signals (contract C20). A message wake carries only
`t`, `channel_id` and `count`; a call wake only routing ids and the call state
(see "Call wakes"). The app fetches content over its normal authenticated
channel.

## Files

- `mod.rs` - `WakeSignal`, the `PushPayload` trait (FCM data, UnifiedPush
  body, priority, collapse key, TTL), `deliver` / `deliver_with`, `send_one`
  (provider dispatch), UnifiedPush transport, subscription CRUD and ownership
  (who may overwrite or delete a row, what happens when its session ends).
- `call.rs` - `CallWake` (C20 addendum "call wake") and `CallPusher`, which the
  DM call service uses to send `ringing` and `ended`.
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

## Security notices (C28)

- `t=security` (`security_events::push::SecurityPush`), sent by
  `security_events::notices` for every new notice in the Kaiku DM and at most
  once per notice per 60 s while it is updated. FCM data (strings):
  `{"t":"security","notice_id","event_type","severity":"high"|"normal","count":"<n>","subject_id"}`,
  plus `actor_id` only when an admin or the recovery contact acted;
  UnifiedPush: the same JSON with `count` as a number.
- Always HIGH, `collapse_key` = `security-<notice_id>`, TTL `86400s`. Sent to
  every subscription of the recipient through `deliver_with`, outside the C20
  coalescer and the notification settings (the server has no quiet hours;
  clients must not suppress `high`). No content beyond these fields; no
  notice carries an action that approves anything. Kaiku DM messages never
  produce a `t=dm` wake (they are not sent through the message pipeline).

## Call wakes (C20 addendum)

- Payload (FCM data-only, every value a string; UnifiedPush gets the same
  JSON): `{"t":"call","channel_id":"<dm>","call_id":"<uuid>","caller_id":"<uuid>","call_state":"ringing"|"ended","reason":"..."}`.
  `reason` only for `ended`: `no_answer`, `all_declined`, `cancelled`,
  `last_left`. `caller_id` in both states. No display name, content or count;
  there is no `answered` state.
- FCM: `priority` HIGH for both states, `collapse_key` = `call-{channel_id}`
  (message wakes and `read` use the bare channel id), TTL `45s` (`ringing`) /
  `3600s` (`ended`).
- Hooks live in `voice::call_service::CallService`, which holds a
  `CallPusher` when built with `CallService::for_state` or `with_push` (the
  SFU leave hook gets the push service through `SfuServer::with_call_push`).
  `ringing` is spawned when `start_call` creates the call (the moment
  `incoming_call` is published); `ended` is spawned on every successful
  transition into `Ended` (decline-all, ring timeout, cancel, last leave,
  caller disconnect, orphan cleanup). Both use the `call_id` of the call the
  service acted on, never a re-read, so a newer call is never confused with
  the one that ended.
- Recipients of `ringing`: the call's targets (DM participants except the
  caller) minus users blocked in either direction (DB check, like DM wakes)
  minus callees over their ringing budget (targets are deduplicated first). That set is stored as JSON in Valkey
  `call_push_rung:{call_id}` (7 h, covering the 6 h active-call backstop).
- `ended` goes only to the stored set, exactly once per `call_id`: it swaps the
  set for a tombstone with one `SET ... GET` (tombstone 1 h). `ringing` stores
  its set with `SET NX`, so an `ended` that wins a race against a slow
  `ringing` task suppresses that ring. A user added to the DM later, or not
  rung, gets no `ended`. A call that lasted longer than 7 h sends no `ended`.
- Rate limiting: call wakes skip the coalescing window and never touch the
  message budget (`USER_MAX_PER_MINUTE`). `ringing` has its own budget per
  callee, `CALL_RINGING_MAX_PER_MINUTE` (10, `Coalescer::admit_ringing`). A
  callee over it gets no `ringing`, is left out of the stored set and so gets
  no `ended` either. `ended` is never rate-limited: it only goes to callees
  whose `ringing` went out. Slots are reserved before the set is stored and
  given back (`refund_ringing`) if the store fails or finds the tombstone, so a
  suppressed ring consumes no budget.
- Ordering: the ringing task holds a per-call in-process lock
  (`PushService::call_lock`, taken before the set is stored) until all its
  deliveries finished. `ended` waits for the same lock before sending (each
  send is bounded by its HTTP timeout), so `ended` is always the last wake
  the server sends for a call: a phone gets nothing, or `ringing` then
  `ended`, never `ended` then `ringing`. FCM still does not guarantee delivery order, so clients must match
  `call_id` and state and ignore a `ringing` for a call they already know
  ended. Single replica, like the coalescer.
- Best effort: all of this runs in spawned tasks; failures are logged with the
  channel or user id only.

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
  The delete on `Gone` and the `last_used_at` update on success apply only
  while the row still has the provider and endpoint the wake was sent to, so a
  late answer for an old token never touches a row that moved to a new one.
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
- Call wakes: message-builder unit tests in `fcm.rs`, UnifiedPush body in
  `mod.rs`, ringing budget/refund in `coalesce.rs`, end to end through the FCM
  mock in `tests/integration/call_push.rs` (`CallPusher::ringing_hooked` is the
  seam for the ordering test). UnifiedPush delivery is not tested end to end:
  a local mock fails the https + SSRF checks by design.
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
(C19 mandates this). Within an account, a full session may overwrite any row by
naming its `device_id` (by design; chat-only sessions may not, see
"Ownership").

Endpoints (FCM tokens included) are capped at 2048 bytes: they sit in btree
indexes, whose entries are limited to about 2.7 KB. Registration takes two
advisory locks in a fixed order: the per-user lock (seed 65,
`push_register_user`), then the (provider, endpoint) lock (seed 67,
`push_register_endpoint`); both are in the `db/mod.rs` registry.

## Ownership (C27, chat-only sessions)

A subscription row belongs to the session that registered it
(`push_subscriptions.session_id`, no foreign key; a missing or ended session
counts as "no longer live").

- `GET /` and `DELETE /{id}` need a full session (`require_full_session`
  layer). `POST /` and `DELETE /device/{device_id}` are open to every session.
- `create_subscription` locks the caller's session row (`FOR SHARE`, after the
  two advisory locks) and refuses with 401 `INVALID_TOKEN` when it is no
  longer live. Lock order with the session-end cascade: session row, then
  push rows, on both sides.
- Two kinds of rows. A key phone's notification registration lives as long
  as its key device; every other registration lives as long as the sign-in
  that made it.
- *Key phone's row* (`key_phone_row!`): its install id is the install of an
  active or pending key device of the account. No session end touches it
  (`delete_for_ended_sessions` skips it for every reason); it goes with the
  device's revocation (`RevokeReason::sends_last_notice` → `after_revoke`
  after the last notice; any other reason in `revoke_tx`; a pending phone an
  account request withdraws or replaces in `account_requests::core`, same
  transaction) and with the account. The condition has one definition
  (`key_phone_row!`), also used by the pre-registration check. Only a full caller may register, overwrite or delete it: a
  chat-only registration naming such an install id → 403
  `FULL_SESSION_REQUIRED`, also when no row exists yet; delete-by-device skips
  it; the same-endpoint check and the pruning never take it.
- *Every other row* belongs to the session that registered it and is deleted
  in the transaction that ends that session, for every end reason (no keep
  list). A key-device session ended with `authenticator_revoked` leaves the
  rows of its own install to the revocation (the device is already marked
  revoked at that point, so they are no longer a key phone's rows); a
  re-enrolment of the same install keeps them (the new key is active).
- A chat-only caller may overwrite (same `(user, device_id)` or same
  `(user, endpoint)`) or delete only rows it may take (`takeable_by!`): not a
  key phone's, and its own, without owner, or of a session that is no longer
  live. Upsert otherwise → 403 `FULL_SESSION_REQUIRED` with nothing changed;
  delete-by-device skips the row (still 204). A full caller may overwrite or
  delete any row of the account and becomes its owner.
- Pruning: a chat-only session holds at most
  `MAX_SUBSCRIPTIONS_PER_CHAT_ONLY_SESSION` (2) rows; the per-account cap never
  prunes a key phone's row or another live session's row when the caller is
  chat-only (the account may then stay over the cap for a while; it is bounded
  by the session limit).
- A device-key sign-in (`approvals::device_login`) takes over the install's
  rows that belonged to a key-device session of that install, or to none
  (`adopt_install_rows`); a row another session registered under that install
  id stays with it. It does not take the registration's advisory lock (that
  would invert the lock order with the superseded session's row); with the
  rules above the owner of a key phone's row only matters once the device is
  revoked, and every revocation path removes the install's rows regardless of
  the owner.
- "End all other sessions" (`auth::sessions::revoke_all_other_sessions` →
  `delete_all_but_session_tx`) deletes every row of the account that is not a
  key phone's and not the acting session's, whoever owns it.
  `db::cleanup_expired_sessions` deletes the removed sessions' rows (never a
  key phone's) in its transaction.
- Deliveries ignore the owner. A phone whose row went with an ended sign-in
  asks again when its app next starts or its address changes, not earlier than
  a day after its last registration (the released app keeps its local record
  24 h).
- Residual: an address registered under an install id before that install
  became a key phone (first setup, a removed phone set up again, rows from
  before the upgrade) counts as the key phone's row; session ends and "end all
  other sessions" do not remove it. The phone's next own registration from its
  full session replaces it, and removing the phone deletes it.

## Single replica

The access-token cache and the coalescer (windows, per-user cap, read gates)
are per process and assume a single server replica. Several replicas would
each coalesce and rate-limit on their own (more pushes, read gates that miss);
that needs shared state (e.g. Valkey) first. A restart forgets pending
trailing pushes and read gates.
