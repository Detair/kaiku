# Kaiku: "Set up this PC" (v1.0)

**Status:** Draft for maintainer review, 2026-11-06. Not yet reviewed. It amends
[the phone-as-key login spec, revision 5](2026-11-02-phone-key-login-and-simple-app-design.md)
("the login spec"); terms such as probation, key device, `may_approve`, F1, F2, F5a, SM1, SM2 and B.7
mean what they mean there.

**Revision 3, 2026-11-06:** design review rounds 1 and 2 resolved; see the appendix. The setup token is
gone, on the desktop the code is typed in the browser, and "I didn't start this" no longer touches
credentials.

**Decided 2026-11-06 by the maintainer:** D1 option A (code from phone to PC), D2 as recommended
(`may_approve` default unchanged). Calls the author made after the builder check are marked
**(call)**; reviewers should challenge them first.

## Intent

A person sits at a PC whose session is chat-only and wants full access on that PC. Today they must
think of the phone by themselves, open Settings → Security → Add a phone (although they are not
adding a phone), choose Share link, get the link to the PC, open it in a browser, create the passkey,
and type a code from the PC into the phone. The maintainer hit this in the first real rc.4 test.

The new flow starts where the person is:

1. The PC's banner offers **Set up this PC**, and the PC shows a QR code.
2. The person scans it with the Kaiku app. The phone says what is about to happen and asks for the
   screen lock.
3. The person types the code the phone shows on the PC: in the page on the web, in the browser the app
   opens on the desktop.
4. A browser creates the passkey, and the person signs in with it. The session is now full.

## Why this needs care

The login spec deliberately has no such path. Open question 5 says a key-device path for PCs "would
need a proximity check that Kaiku does not have", and two anti-goals are close to this flow:

- "An attacker tricks a user into scanning *the attacker's* QR code";
- "A stolen browser or desktop session becomes a permanent takeover: it adds its own key device or
  passkey".

The danger is concrete. F2's QR scan can be relayed, which is acceptable because it yields only a
probation session. The same scan here would yield a passkey, which means sign-in changes, ending the
owner's sessions and, on an admin account, the admin panel. An attacker holding a stolen or phished
probation session could send the victim their QR code ("scan this to fix your Kaiku"), and a fooled
scan plus a fingerprint would turn it into a takeover.

F5a is safe against this because its secret starts on the trusted phone and the person has to hand it
over. A flow that starts on the PC reverses that, so it needs something to replace it.

## D1: the code typed on the PC (decided: option A)

| Option | Fooled person must | Effort for the person | Assessment |
|---|---|---|---|
| **A. Code from phone to PC (chosen)** | scan, confirm with the screen lock, **and** read a code from their phone to the attacker or type it into the attacker's page | one 6-digit code | Comparable to F5a, plus F5a's attacker needs no prior session here |
| B. Scan and screen lock only | scan and confirm | none, as first proposed to the maintainer | One fooled scan turns a stolen session into a takeover; contradicts the second anti-goal |
| C. Scan and screen lock, then 24 h wait with Cancel | scan and confirm, then nobody cancels for 24 h | none, but the PC stays chat-only until tomorrow | Same delay as "Secure my account"; defeats the purpose for the person at the PC |

A code from the PC typed on the phone adds nothing: it travels in the same direction as the QR code,
so whoever supplies the one supplies the other.

Limits of option A, stated honestly: it is not a proximity check. A live phishing page, or a caller
who talks the person through it, still gets through. Passkeys over hybrid remain the only
phishing-resistant cross-device path, and this flow does not replace them.

## D2: who gets the button (decided: default unchanged)

`may_approve` is off by default for accounts an admin sets up (login spec F5c, open question 4), and
the button needs it on. **Decision:** keep that default, so the flow serves people whose phone may
already confirm sign-ins, and an admin turns the existing switch on for a relative who needs it. No new
admin UI. The rejected alternative, on by default for family accounts, would make this flow and QR sign-in
available to everyone and widen the residual risk below to the people least likely to spot a trick.

## Goals

- A person with a key device gets a passkey on this PC without finding a phone menu or moving a link.
- A stolen probation session alone gets nothing, and neither does a QR photo. There is no setup link
  in this flow to lose: no secret that creates a passkey ever leaves the requesting client's session.
- Every use is visible afterwards and can be undone with "That wasn't me".
- It works without push notifications. A server without FCM is a supported setup.

## Non-goals

- Upgrading a session: a session's assurance never changes (login spec B.4). The person ends up with
  a new, full session from a passkey sign-in.
- A proximity check, or any network signal used as one.
- A flow for a PC with no session. The login screen is unchanged: such a PC first signs in by F2.
- A flow for accounts without an eligible key device. They keep the admin setup link and "Secure my
  account".
- Registering a passkey inside the desktop app's webview. Registration stays in a browser, and F1 uses
  the same system browser, so a passkey stored there works for the sign-in in step 6.
- Changing F5a. "Add a phone → Share link" stays as it is.
- The simple app (v1.2).

## Anti-goals

- A prompt that reaches a phone without a scan (login spec CC2, "nothing to bomb"). No push flow.
- A phone screen that reads like a routine sign-in. This grants far more than F2 and must say so.
- A person stuck halfway with a used-up code and no clear way back.
- Any secret in a URL query, a log or the audit trail.

## Who sees the button

**Set up this PC** appears in the probation banner and in the empty passkey list of the security
settings, on the web and in the desktop app, when:

- the session is probation; and
- the account has at least one active key device with `may_approve` on. The session reads this from
  a new field in `/api/me/security`, e.g. `can_set_up_this_pc`.

If the account also has an active passkey, **Sign in with a passkey** stays the first action and this
button comes second: that passkey may be on a security key that works here, or it may not **(call)**.

Otherwise the existing text stays, shortened to the case that applies: no eligible phone → ask your
admin. `auth.probation.addPasskeyHint` no longer describes the Share-link path in the banner; the user
guide still does.

## The flow

### Step 1: the PC starts

The session calls a new endpoint and receives an attempt id and a QR token. It needs a live probation
session of the account, and the server refuses the start unless the account has an eligible key device,
so an attempt can never wait for a phone that does not exist. The route is an exception to the probation rules in B.4 and needs the login
spec amended (below): the session asks, and only the phone can grant.

The QR holds the server origin and a 256-bit token, with a payload type distinct from F2 and F5a, so
that a scanner can never mistake one for the other. It rotates every 60 s like F2. Only keyed hashes
are stored. The attempt is bound to the session's `sid`, and one session has at most one live attempt:
a new one ends the old.

Collection needs no extra secret: every attempt route needs the bearer session of that `sid`, and
another session of the same account gets 404. A QR photo therefore yields nothing.

### Step 2: the phone scans

The scan happens in the Kaiku app's own scanner, from the phone's live session, on an active key
device with `may_approve` on. The app checks the origin against its pinned server. The payload is a new
shape (for example `kaiku://pcsetup/v1`) and has its own route; `POST /api/approvals/claim` stays
F2-only **(call)**. An installed app without this feature reports "not a Kaiku code", which is
acceptable because there are no released v1.0 apps yet **(call)**.

The server finds the attempt by the token and refuses unless **the attempt's account is the phone's
account**. A foreign account gets its own answer, "This computer is signed in with a different
account", with no account name; a person with two accounts would otherwise rescan forever **(call)**.
Scanning an attempt that is not live gets the same answer as a rotated or unknown token. The scanner is
an authenticated key device on the same server, so the distinct answer tells it nothing worth hiding.

### Step 3: the context screen and approval

The screen says what is granted, not just what is signing in:

> **Give this computer full access to your account?**
> Firefox on Linux, signed in 3 days ago. [Approximate place, if the operator has GeoIP.]
> It could change your sign-in methods and sign you out elsewhere.
> Only continue if you started this on a computer in front of you, and nobody asked you to.

The client description and the age come from what the server stored when the session was created,
never from the request that starts the attempt, so a session holder cannot choose them **(call)**. With
a stolen session this honestly describes the victim's own browser and reads as reassuring. The context
screen cannot tell these cases apart; the code step and the phone's wording are the control.

The actions are **Yes, continue**, a prominent **No**, and a smaller **I didn't start this**.

- **Yes** needs UV and a signature over a new challenge kind, `pc_setup`, with the scanned token as
  `response`. It follows B.2 unchanged: the phone recomputes the context hash from what it shows.
  Canonical context: attempt id, `sid` hash, client description, session `created_at`, display fields.
- **No** ends the attempt. It needs no signature, because it has no effect beyond that.
- **I didn't start this** first asks "End that computer's session?". It then needs UV and a signature
  with `decision = report` over the same challenge, with the scanned token as `response`, like every
  report in B.2. It ends the attempt and the requesting session and records a high-severity event. It
  can only end the session that showed the QR.

  It does **not** mark any password or TOTP compromised **(call)**. F3's report does, because a correct
  password was typed at that moment. Here the session may be days old, and a stolen session says nothing
  about the password. Otherwise a thief could make a careful owner destroy their own password just by
  getting them to scan and report. The notice says what happened and offers the existing paths: the
  security log's "That wasn't me" on the sign-in that created that session, and changing the password.

### Step 4: the code

On approval the server generates a 6-digit code, stores its keyed hash and returns the digits to the
phone only. The phone shows:

> **Type this code on the computer in front of you.**
> **Never tell it to anyone or type it on a site someone sent you.**

The server never sends the code again. The app keeps it in memory while the attempt is in `code`, so
locking the phone or switching apps does not lose it. If the app process dies or the response is lost,
the person presses **Start again** on the PC, which is always available, and approves anew **(call)**.
The phone polls the attempt while it shows the code, at most once a second, so it can say "Done",
"Cancelled" or "Too many wrong codes" without push.

The PC, polling over its session, sees `code` and asks for the code (where: step 5). The code is valid
for 5 minutes. Three wrong
entries end the attempt, and the phone and the PC both say so. There is no per-account counter, as in
B.5: a wrong entry can only be made by the session that holds the attempt, and each attempt needs a
new approval on the phone.

### Step 5: the passkey

The code is entered where the passkey will be created, so that the browser registering it is proven to
be with the person holding the phone.

The right code moves the attempt to `enrolled` and creates an enrolment (SM2, new variant `pc_setup`)
with a new `issuer_kind` recording both the approving key device (its `approval_devices` row id) and the
requesting session. If that response is lost, the PC's next poll sees `enrolled` and carries on.

**There is no setup token.** The login spec's setup token is a bearer secret, safe in F5a only because
the claimant must then pass a code back to the issuer. Here the code has already been used, so a bearer
token could be raced by anyone who saw it. Registration is bound to the requesting session instead:

- **Browser client:** the page shows the keypad. It enters the code and then calls session-authenticated
  `passkey/start` and `passkey/finish` routes of the attempt, reusing LP's `register::{begin, complete}`.
  The registration and the session are in the same page. The `/setup` route is not involved.
- **Desktop app:** when its poll sees `code`, the app starts a new purpose of the existing F1 desktop flow
  (`server/src/auth/passkeys/desktop.rs`), e.g. `FlowPurpose::PcSetup { user_id, session_id, attempt_id }`,
  with its session's bearer, the challenge and the loopback port, as `step_up` does. It opens the system
  browser at the flow's URL and says "Type the code in your browser". The server's page there asks for
  the code **before any ceremony**: `options` refuses until the right code has been entered through that
  flow, and wrong entries count on the attempt.
  - The hand-off then works as in F1, and **nothing takes effect before `redeem`**, with the one-time
    code, the verifier and the starting session's bearer.
  - The flow URL alone is therefore worthless. Whoever has it but not the phone's code cannot reach a
    ceremony, so they cannot produce a hand-off for the app to redeem (round 2's relay attack). They can
    only burn the three tries, which ends the attempt and shows on the PC and the phone.
  - F1 today supports assertions only. Registration needs a new ceremony kind, pending result and
    redemption handling, which holds the verified credential until `redeem`.

At `done` the passkey row is created as `active`. A failed or cancelled ceremony can be retried until
the enrolment expires (10 minutes). Firefox on Linux gets the existing `LinuxPasskeyHint` guidance
first, and the PC offers **Start again** at every point.

### Locking (normative)

These rules extend B.7. Reviewers should check them against `server/src/approvals/cascade.rs`.

1. Attempts stay out of the session-ending cascade. Logout and refresh hold a session row without the
   account lock, and they must keep touching only sessions, grants and enrolments. A session end or a
   device revocation therefore leaves the attempt row as it is, and:
2. Every attempt transition (scan, approve, report, code entry, cancel, restart, finish, redeem) takes
   the account lock first, then re-checks under it that the requesting session is live and that the
   device is active with `may_approve` on. If not, it moves the attempt to `expired` with a reason the PC
   can read. The minute sweep expires what no transition reaches. A stale attempt can therefore never
   advance, even before the sweep.
3. Code entry, finish, redeem and restart all lock account → requesting session row → attempt → enrolment,
   and hold the session lock through commit. A concurrent session end either comes first, and they see it,
   or comes after, and its cascade sees the new enrolment and the `done` state. They must not reuse
   enrolment confirmation's enrolment-then-session order, which would deadlock with logout.
4. The enrolment rides both existing cascades: ending the session or revoking the device voids it in the
   cause's transaction.
5. A report's `end_sessions_tx` runs in the answer transaction under the account lock, after the challenge
   and attempt locks. Since session-ending paths without the account lock never touch attempts or
   challenges, this cannot form a cycle with them.
6. **Start again** runs under the account lock: it cancels the previous attempt and its `issued` enrolment
   and voids that enrolment's open ceremonies and desktop flows. Finish and redeem take the same lock.
   Either the finish wins and the PC reports completion, or the restart wins and the old finish fails.

### Step 6: signing in

The PC shows **Sign in with your passkey**, which runs F1 and replaces the probation session
(`replaceSessionWithPasskey`). The session's assurance is never changed in place.

### Notices and audit

The existing "new passkey" high-severity notice goes to all addresses (devices, recovery contact,
admins), names both the PC and the approving phone, and carries "That wasn't me". For this flow the
event records `via: pc_setup` and the requesting `sid`. **"That wasn't me" revokes the passkey, ends its
sessions and also ends the requesting session**, because here the requesting session is the suspect, not
a trusted issuer as in F5a. It marks no credential, for the reason given under step 3. Additional
events, coalesced under B.5:

- `pc_setup_reported` (high severity) for "I didn't start this";
- `pc_setup_failed` for three wrong codes. It goes to the account only.

Starting an attempt, approving it and refusing it are audit-logged; the rows carry no token or code.

## State machine (normative, added to B.7 as SM5)

| From | Event | Who may trigger | To | Rules |
|---|---|---|---|---|
| – | start | live probation session with an eligible key device | `waiting` | one live per session; a new start is a restart (Locking 6); token rotates every 60 s while `waiting`; ≤10 min |
| `waiting` | scan | eligible key device of the same account | `claimed` | creates the `pc_setup` challenge, TTL 2 min; rotation pauses; the signed token stays the scanned one |
| `claimed` | release, or the challenge expires | that device, or server | `waiting` | the challenge is voided; as in SM1 |
| `claimed` | approve | that device, with UV | `code` | challenge consumed in the same transaction; code 5 min; the code goes only to the device |
| `claimed` | No | that device | `refused` | |
| `claimed` | report | that device, with UV | `reported` | ends the requesting session |
| `code` | right code | the requesting session (web), or its desktop flow's page | `enrolled` | enrolment `pc_setup` created in the same transaction |
| `code` | third wrong code | the same | `failed` | `pc_setup_failed` |
| `enrolled` | enrolment `done` | passkey finish (web) or desktop redeem, by the requesting session | `completed` | passkey `active` |
| `enrolled` | enrolment cancelled or expired | the requesting session, or server | `cancelled` / `expired` | |
| `waiting`, `claimed`, `code` | cancel or restart | the requesting session | `cancelled` | the phone says it was cancelled |
| `waiting`, `claimed`, `code` | timer, or the session or device is no longer valid | server | `expired` | with a reason the PC can read |

Every state except `waiting`, `claimed`, `code` and `enrolled` is terminal.

**Timers (call):** the 10-minute bound covers `waiting` and `claimed`. The code's 5 minutes run from the
approval, and the enrolment's 10 minutes from the right code, each on its own clock. B.2's 10-minute limit
binds challenges, and the only challenge here lives 2 minutes. The longest run is about 25 minutes.

## Changes to the login spec

To be applied with this feature, as revision 6:

- **B.2, kinds table:** add `pc_setup` as an approval (Requester: the attempt; answering device: the
  scanning key device, `may_approve` on; `response`: the QR token; TTL 2 min from the claim).
- **B.2, remediation table:** add `authenticator_added` via `pc_setup`: revoke the passkey, end its
  sessions and the requesting session; add `pc_setup` report: end the requesting session. Neither marks a
  credential.
- **B.4:** probation may start this attempt, enter its code and register the passkey it unlocks, and
  nothing else about it. "Create setup codes" stays forbidden: no setup code exists here.
- **SM2:** add variant `pc_setup` (issued by: an attempt whose code was entered, recording device and
  session; no setup token and no claim, so the table's only route is SM2's existing "registration on the
  issuing client itself" row; authenticator: passkey only, `active`). The builder decides how the
  `setup_token_hash NOT NULL` column accommodates a row with no token.
- **B.7:** add SM5 and the locking rules above.
- **C29:** add the desktop flow purpose.
- **F5:** add **F5e**, pointing to this document.
- **B.6:** add a residual risk: a person fooled into scanning and passing on the code gives an
  attacker who already has a session on the account a full passkey. Mitigated by the account match,
  `may_approve` off by default for family accounts, the phone's wording and the notices with "That
  wasn't me". Same class as B.6.1 with a higher impact.
- **Open question 5:** record the answer. Admin work on a PC still needs a passkey, and this flow is a
  way to get one, not an exception.

C26 (kind byte, canonical context, golden vectors) and C27 (routes, error codes) get the additive
entries. The user guide, admin guide and technical reference under
`docs/admin-guide/features/login/` describe the flow, and the admin guide states the residual risk.

## Error cases that must read clearly

| Situation | PC says | Phone says |
|---|---|---|
| Phone belongs to another account | keeps waiting | "This computer is signed in with a different account." |
| QR rotated, unknown or not live | new QR | "This code isn't valid or has just changed. Scan the code on your computer again." |
| Scanned and abandoned | new QR after 2 min | nothing |
| No eligible phone at start | "Your phone can't confirm this yet. Ask your admin." | nothing |
| `may_approve` turned off meanwhile | "Your phone can no longer confirm this. Ask your admin." | same refusal as F2 |
| Phone says No | "Your phone declined. Start again?" | returns to the app |
| Session ended meanwhile | signed-out screen | "That session has ended." |
| Too many passkeys, or already registered | existing passkey error text | "Done" is not shown; "That didn't work." |
| Browser never finished | "This took too long. Start again?" | nothing |
| System browser cannot be opened | shows the existing "open in your browser" guidance | nothing |
| Passkey on this browser impossible | existing `passkeyFailureText` and Linux hint | nothing |

## Constraints

- Server: `server/src/approvals/` core, no forked approval logic; additive migration; `.sqlx` updated.
- Android: reuse `ui/scan/` and the login approval screen pattern; the code display follows the
  existing setup screens' size and contrast.
- Web and desktop: reuse `CodeEntry`, `ProbationBanner` and the desktop flow client; strings in the i18n
  files.
- No private hostnames or IPs in the public repository.
- An auth PR. Needs the maintainer's explicit merge OK, and reviews by Codex, Flash and Fable as the
  largest late auth change of v1.0.

## Tests that must exist

Server:

- `pc_setup_needs_a_live_probation_session` and is refused from a full session;
- `start_refused_without_an_eligible_key_device`;
- `scan_by_other_account_gets_its_own_answer_without_a_name`;
- `may_approve_off_refuses_scan_and_approval`;
- `other_session_of_the_account_gets_404_on_every_attempt_route`;
- `code_is_returned_to_the_device_only`: no PC response and no log line contains it;
- `third_wrong_code_ends_the_attempt`, `expired_code_refused`;
- `no_route_returns_a_setup_token_for_pc_setup`;
- `registration_needs_the_requesting_session`: browser routes refuse another session and no session;
- `desktop_redeem_needs_code_verifier_and_the_starting_session`;
- `desktop_flow_options_refused_before_the_right_code`;
- `relayed_handoff_from_a_leaked_flow_url_is_impossible`: without the phone's code there is no ceremony,
  so no hand-off exists to relay to the legitimate app;
- `lost_code_entry_response_recovers_by_poll`;
- `session_ended_or_device_revoked_stops_every_transition`, before the sweep has run;
- `ending_the_session_or_revoking_the_device_voids_the_enrolment`;
- `report_needs_uv_and_signature`; `no_needs_no_signature_and_only_ends_the_attempt`;
- `report_ends_only_the_requesting_session_and_marks_no_credential`;
- `that_wasnt_me_revokes_the_passkey_and_ends_the_requesting_session`;
- `notice_names_pc_and_phone`;
- `challenge_expiry_returns_to_waiting`;
- `restart_cancels_the_previous_enrolment_and_its_ceremonies`; `restart_racing_finish_has_one_winner`;
- `context_comes_from_the_session_row_not_the_request`;
- `pc_setup_challenge_signature_golden_vectors`: approve and report;
- lock-order tests: logout against report, logout against finish and redeem (the cascade must not miss a
  `done` enrolment), refresh-reuse revocation against code entry, and device removal against finish.

Client (web and desktop): the desktop app never shows a keypad, button visibility by `can_set_up_this_pc`, each row of the error table, Start
again at every step. Android: context-screen wording, refusal for a foreign account, the code shown only
after UV and still shown after locking the phone.

Device checks, added to the existing list: one full run per platform (Windows, macOS, Linux with
Chromium, Linux with Firefox and a security key, browser only, desktop app), plus a run on a server
without FCM.

## Left to the builder

- Table layout: a new attempt table or an extension of the login-attempt table, as long as SM1's
  behaviour stays untouched.
- Exact route names and the QR payload encoding, within C26/C27's conventions.
- Poll or long-poll for the PC, reusing the enrolment status pattern (never more than once a second).
- PC label wording beyond the client description F2 already has.
- Final copy, within what this document requires it to say.

## References

- Login spec: anti-goals, CC2, CC3, B.2, B.4, B.5, B.6, F2, F5a, SM1, SM2, open question 5.
- `client/src/components/auth/ProbationBanner.tsx`, `server/src/auth/passkeys/desktop.rs`,
  `server/src/approvals/cascade.rs`, `server/src/approvals/enrollments/`, `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/scan/`.

## Appendix: design review round 1

| Finding | From | Resolution |
|---|---|---|
| Setup token after the code is an unbound bearer secret | Codex, Flash (Blocker), Fable M4 | Token removed; registration bound to the session or the desktop flow's redeem |
| "That wasn't me" leaves the requesting session alive | Fable M1 | It now ends that session |
| Report marks credentials: wrong generation, voids requests, self-lockout | Flash (Blocker), Codex, Fable M3 | The (call) is reverted: no credential is marked |
| Report needs signature and UV | Codex | Specified |
| Attempt in the session cascade collides with B.7 | Fable M2 | Attempts stay out; every transition re-checks under the account lock |
| Attempts missing from the cascades | Flash (Blocker) | Rejected as stated: a stale attempt cannot advance because of the re-check (Locking 2); the enrolment does ride the cascades |
| Issuance race with refresh-reuse revocation | Codex | Locking 3 |
| `enrolled` has no exit, restart leaves the child alive | Flash, Codex, Fable m4 | SM5 rows, Locking 6 |
| Identical answer traps a person with two accounts | Flash | The (call) is changed: own answer, no name |
| Passkey in the browser unusable from the desktop app | Flash | Rejected: F1 runs in the same system browser |
| 25-minute chain violates B.2's 10 minutes | Flash | Rejected: B.2 binds challenges; the only challenge here lives 2 minutes |
| Lost code entry response | Flash | Poll sees `enrolled`; no secret to re-send |
| Code lost on phone lock | Flash | Kept in memory while the app lives |
| Challenge expiry, eligibility at start, phone polling, readable reasons | Codex, Fable m1–m3, m5 | Specified |

## Appendix: design review round 2 (Codex: NOT READY)

| Finding | Resolution |
|---|---|
| Desktop: an attacker with the flow URL registers, relays the hand-off through the victim's browser, and the legitimate app redeems the attacker's credential | Code entered in the browser before any ceremony |
| Finish and redeem lock order deadlocks or misses logout | Locking 3 now covers them and restart |

Codex confirmed in this round: the web path closes the leaked-link attack, and reverting the credential
remediation is safe.
