# Kaiku: "Set up this PC" (v1.0)

**Status:** Implemented. It amends
[the phone-as-key login spec, revision 5](2026-11-02-phone-key-login-and-simple-app-design.md)
("the login spec"); terms such as probation, key device, `may_approve`, F1, F2, F5a, SM1, SM2 and B.7
mean what they mean there.

**Revision 4, 2026-11-06:** the setup token is gone, on the desktop the code is typed in the browser,
and "I didn't start this" no longer touches credentials.

**Implemented 2026-11-08** (plan `docs/superpowers/plans/2026-11-06-this-pc-setup.md`). The amendments
below are applied to the login spec as revision 6 and to C26, C27 and C29; the user guide, admin guide
and technical reference under `docs/admin-guide/features/login/` describe the built flow.

**Decided 2026-11-06 by the maintainer:** D1 option A (code from phone to PC), D2 as recommended
(`may_approve` default unchanged). Calls the author made after the builder check are marked
**(call)**.

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
need a proximity check that Kaiku does not have", and two anti-goals are close to this flow. The flow therefore requires:

- the person who scans the QR code holds the phone and sees the computer the code is for: the phone
  shows what is about to happen, the screen lock signs the confirmation, and the person also types the
  code the phone shows on that computer, so a scan alone never enrols a passkey;
- a browser or desktop session that is only signed in never becomes a permanent sign-in by itself: the
  server refuses to give it a key device or passkey of its own without the phone's confirmation and the
  typed code.

Why a scan is not enough here: F2's QR sign-in yields only a probation session, so its confirmation
may stand for a computer the person does not see. Here the same confirmation would lead to a passkey,
which means sign-in changes, ending the owner's sessions and, on an admin account, the admin panel. So
the server never gives a chat-only session a passkey for a scan and the screen lock alone: the person
at the phone also types a code on the computer in front of them.

F5a needs no such step because its secret starts on the trusted phone and the person moves it on purpose.
A flow that starts on the PC reverses that direction, so it needs a deliberate step of its own.

## D1: the code typed on the PC (decided: option A)

| Option | Before a passkey is created, the person must | Effort for the person | Assessment |
|---|---|---|---|
| **A. Code from phone to PC (chosen)** | scan, confirm with the screen lock, **and** type the code the phone shows on the computer | one 6-digit code | A deliberate step on each device, comparable to F5a |
| B. Scan and screen lock only | scan and confirm | none, as first proposed to the maintainer | One confirmation would give the session that showed the code a passkey; contradicts the second anti-goal |
| C. Scan and screen lock, then 24 h wait with Cancel | scan and confirm, then nobody cancels for 24 h | none, but the PC stays chat-only until tomorrow | Same delay as "Secure my account"; defeats the purpose for the person at the PC |

A code from the PC typed on the phone adds nothing: it travels in the same direction as the QR code,
from the same screen.

Trust limits of option A: the code shows that whoever typed it got it from the person at the phone; the
server cannot tell where that person stands. It is not a proximity check. The phone therefore says to
type the code only on the computer where this was started, and every passkey made this way is
announced with "That wasn't me". Passkeys over hybrid remain the only cross-device path with a
proximity guarantee, and this flow does not replace them.

## D2: who gets the button (decided: default unchanged)

`may_approve` is off by default for accounts an admin sets up (login spec F5c, open question 4), and
the button needs it on. **Decision:** keep that default, so the flow serves people whose phone may
already confirm sign-ins, and an admin turns the existing switch on for a relative who needs it. No new
admin UI. The rejected alternative, on by default for family accounts, would make this flow and QR sign-in
available to everyone, including people who never asked for it, and widen the trust limit above to them.

## Goals

- A person with a key device gets a passkey on this PC without finding a phone menu or moving a link.
- A probation session alone gets nothing, and neither does a picture of the QR code: a passkey needs the
  scan, the screen lock and the typed code. There is no setup link
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
never from the request that starts the attempt, so a session holder cannot choose them **(call)**. They
describe the session that showed the code, not who holds it now; the context screen cannot tell that,
so the code step and the phone's wording are the control.

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
  password was typed at that moment. Here the session may be days old, and a report about it says nothing
  about the password, so a report must not cost the owner their own password. The notice says what happened and offers the existing paths: the
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
requesting session. On the web, if that response is lost, the PC's next poll sees `enrolled` and
carries on. On the desktop, if the browser does not receive that answer, the browser page cannot
continue: "Start again" on the PC begins a new attempt, which the phone approves again; otherwise the
attempt ends by itself when its time runs out.

**There is no setup token.** The login spec's setup token is a bearer secret, safe in F5a only because
the claimant must then pass a code back to the issuer. Here the code has already been used, so no
bearer secret is issued at all. Registration is bound to the requesting session instead:

- **Browser client:** the page shows the keypad. It enters the code and then calls session-authenticated
  `passkey/start` and `passkey/finish` routes of the attempt, reusing LP's `register::{begin, complete}`.
  The registration and the session are in the same page. The `/setup` route is not involved.
- **Desktop app:** when its poll sees `code`, the app starts a new purpose of the existing F1 desktop flow
  (`server/src/auth/passkeys/desktop.rs`), e.g. `FlowPurpose::PcSetup { user_id, session_id, attempt_id }`,
  with its session's bearer, the challenge and the loopback port, as `step_up` does. It opens the system
  browser at the flow's URL and says "Type the code in your browser". The server's page there asks for
  the code **before any ceremony**, and wrong entries count on the attempt. The right code answers with
  a **browser secret**: a fresh 256-bit value in an HttpOnly, `Secure`, `SameSite=Strict` cookie scoped to
  the flow's routes, stored only as a keyed hash, in the manner of the existing `__Secure-kaiku_enroll`
  claim cookie. `options` and `finish` refuse without it. Code entry works once per flow, so the
  authorisation belongs to the browser that typed the code, never to the URL.
  - The hand-off then works as in F1, and **nothing takes effect before `redeem`**, with the one-time
    code, the verifier and the starting session's bearer.
  - The flow URL alone is therefore worthless, before and after the legitimate code entry. Without the
    code or the cookie, the URL leads to no ceremony and no hand-off for the app to redeem. Before the code is entered, wrong entries on the page count on the attempt; the third ends it,
    and the PC and the phone both say so.
  - F1 today supports assertions only. Registration needs a new ceremony kind, pending result and
    redemption handling, which holds the verified credential until `redeem`.

**One path per attempt (as built).** The attempt records how the right code arrived: through the
requesting session's own route (`web`), or through one specific desktop flow (its id). Only that path
may register the passkey: the web `passkey/start` and `passkey/finish` refuse an attempt whose code came
through a flow, and a desktop flow's `options`, `finish` and `redeem` refuse an attempt whose code did
not come through that flow. A `pc_setup` desktop flow can be started only by a session whose stored
client class is `desktop`; the request header alone is not enough. Starting in the browser and finishing
in the desktop app (or the reverse) therefore needs **Start again**.

**The web path's binding is the session's bearer.** After the right code the browser client's page
registers with the same session's access token and nothing else. Whoever holds that token at that moment
holds the session anyway (B.4, B.6); the desktop path adds the browser secret because there the code is
typed outside the app.

At `done` the passkey row is created as `active`. A failed or cancelled ceremony can be retried until
the enrolment expires (10 minutes). Firefox on Linux gets the existing `LinuxPasskeyHint` guidance
first, and the PC offers **Start again** at every point.

### Locking (normative)

These rules extend B.7; `server/src/approvals/cascade.rs` implements the cascades they rely on.

1. Attempts stay out of the session-ending cascade. Logout and refresh hold a session row without the
   account lock, and they must keep touching only sessions, grants and enrolments. A session end or a
   device revocation therefore leaves the attempt row as it is, and:
2. Every attempt transition (scan, approve, report, code entry, cancel, restart, finish, redeem) takes
   the account lock first, then re-checks under it that the requesting session is live and that the
   device is active with `may_approve` on. If not, it moves the attempt to `expired` with a reason the PC
   can read. The exception is a scan refused for its scanner (checked before the token is read): it
   writes nothing, and the attempt ends at the PC's next poll or the sweep. The minute sweep expires what
   no transition reaches. A stale attempt can therefore never
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
7. **One clock (as built).** Every transition takes all its locks first and then reads the database's
   `clock_timestamp()` once. That one value decides every liveness it checks (both sessions included)
   and every deadline it issues (the scan's challenge included: its creation time and expiry are that
   value and the earlier of that value plus 2 minutes and the attempt's stored deadline, in whole
   seconds), so a wait for a lock never shortens or extends a deadline:

   | Transition | Locks, in order, before the clock | Decided and issued on that clock |
   |---|---|---|
   | start | account, requesting session, the session's live attempt (and its enrolment) | session liveness, start bounds, the old attempt's lazy end; creation time, QR rotation, 10 minutes |
   | PC poll, cancel, code entry, passkey | account, requesting session, attempt, its enrolment | liveness, QR rotation, code and enrolment expiry; the enrolment's creation and 10 minutes |
   | scan | account, requesting session, attempt; key device, its session | both sessions, QR rotation, attempt; the challenge's creation and expiry |
   | answer, refuse, release | account, requesting session, attempt; key device, its session; challenge | both sessions, attempt, challenge; the code's 5 minutes |

   Where a step reads its own clock instead: a restart voids the old attempt's challenge after its clock
   was read (that write decides nothing); the unlocked reads before the locks (the scanner check, the
   one read of another account's code, the polls' and the sweep's hints) use `clock_timestamp()` of
   their own statement, and the locked re-check decides; the account lock (`lock_accounts`, the ban's
   expiry included) is as before this feature. The phone's steps use the checks of the shared
   key-device session helper but decide the session's expiry on the transition's clock. The other
   challenge kinds keep their timing on the server's clock, unchanged by this feature.

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
- **B.6:** add a trust limit: the passkey is bound to the session that showed the QR code; the server
  cannot tell where the person who types the code is. Before a passkey exists, the person at the phone
  must confirm with the screen lock and type the code on the computer in front of them. Limited by the
  account match, `may_approve` off by default for family accounts, the phone's wording and the notices
  with "That wasn't me". Same class as B.6.1 with a higher impact.
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
- An auth PR. Needs the maintainer's explicit merge OK, and independent reviews, as the
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
- `a_flow_without_the_phones_code_hands_back_nothing`: without the phone's code there is no ceremony,
  so the flow hands nothing back to the app;
- `only_the_browser_that_typed_the_code_continues_the_flow`: `options` and `finish` refuse without the
  browser secret set by that code entry, and a second code entry on the flow is refused;
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

## Design decisions

- No setup token after the code: registration is bound to the requesting session, or to the desktop flow's redeem.
- After the right code, only the browser that typed it may run the ceremony: the code entry sets a browser secret that `options` and `finish` require.
- On the desktop, the flow URL alone does not lead to a registration that the app then redeems: the code is entered in the browser before any ceremony.
- "That wasn't me" ends the requesting session.
- The report marks no credential.
- The report needs a signature and user verification.
- Attempts stay out of the session cascade; every transition re-checks under the account lock. A stale attempt cannot advance because of that re-check (Locking 2); the enrolment rides the cascades.
- The issuance race with refresh-reuse revocation is covered by Locking 3.
- Finish and redeem lock order is covered by Locking 3, which also covers restart; the lock order holds against logout and the cascades.
- `enrolled` has an exit and a restart does not leave the child alive: see the SM5 rows and Locking 6.
- The answer on the phone is its own, with no name, so a person with two accounts is not stuck.
- The passkey in the browser works from the desktop app because F1 runs in the same system browser.
- B.2's 10 minutes bind challenges; the only challenge here lives 2 minutes.
- On the web, a lost code-entry response is recovered by the poll, which sees `enrolled`. On the desktop the browser page cannot continue without that answer; "Start again" begins a new attempt, which the phone approves again.
- The code stays in memory while the app lives, so a phone lock does not lose it.
- Challenge expiry, eligibility at start, phone polling and readable reasons are specified.
