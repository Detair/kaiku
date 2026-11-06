# Kaiku: "Set up this PC" (v1.0)

**Status:** Draft for maintainer review, 2026-11-06. Not yet reviewed. It amends
[the phone-as-key login spec, revision 5](2026-11-02-phone-key-login-and-simple-app-design.md)
("the login spec"); terms such as probation, key device, `may_approve`, F1, F2, F5a, SM1, SM2 and B.7
mean what they mean there.

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
3. The person types the code the phone shows into the PC.
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
- A stolen probation session alone gets nothing, and neither does a QR photo or a lost setup link.
- Every use is visible afterwards and can be undone with "That wasn't me".
- It works without push notifications. A server without FCM is a supported setup.

## Non-goals

- Upgrading a session: a session's assurance never changes (login spec B.4). The person ends up with
  a new, full session from a passkey sign-in.
- A proximity check, or any network signal used as one.
- A flow for a PC with no session. The login screen is unchanged: such a PC first signs in by F2.
- A flow for accounts without an eligible key device. They keep the admin setup link and "Secure my
  account".
- Registering a passkey inside the desktop app's webview. Registration stays in a browser.
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
  a new field in `/auth/me/security`, e.g. `can_set_up_this_pc`.

If the account also has an active passkey, **Sign in with a passkey** stays the first action and this
button comes second: that passkey may be on a security key that works here, or it may not **(call)**.

Otherwise the existing text stays, shortened to the case that applies: no eligible phone → ask your
admin. `auth.probation.addPasskeyHint` no longer describes the Share-link path in the banner; the user
guide still does.

## The flow

### Step 1: the PC starts

The session calls a new endpoint and receives an attempt id and a QR token. It needs a live probation
session of the account. The route is an exception to the probation rules in B.4 and needs the login
spec amended (below): the session asks, and only the phone can grant.

The QR holds the server origin and a 256-bit token, with a payload type distinct from F2 and F5a, so
that a scanner can never mistake one for the other. It rotates every 60 s like F2. Only keyed hashes
are stored. The attempt is bound to the session's `sid`, and one session has at most one live attempt:
a new one ends the old.

Collection needs no extra secret: every later PC step needs the bearer session of that `sid`. A QR
photo therefore yields nothing.

### Step 2: the phone scans

The scan happens in the Kaiku app's own scanner, from the phone's live session, on an active key
device with `may_approve` on. The app checks the origin against its pinned server. The payload is a new
shape (for example `kaiku://pcsetup/v1`) and has its own route; `POST /api/approvals/claim` stays
F2-only **(call)**. An installed app without this feature reports "not a Kaiku code", which is
acceptable because there are no released v1.0 apps yet **(call)**.

The server finds the attempt by the token and refuses unless **the attempt's account is the phone's
account**. A foreign account, a rotated token and an unknown token get one identical answer and one
message, so a scan cannot tell whether a token is live **(call)**.

### Step 3: the context screen and approval

The screen says what is granted, not just what is signing in:

> **Give this computer full access to your account?**
> Firefox on Linux, signed in 3 days ago. [Approximate place, if the operator has GeoIP.]
> It could change your sign-in methods and sign you out elsewhere.
> Only continue if you started this on a computer in front of you, and nobody asked you to.

The client description and the age come from what the server stored when the session was created,
never from the request that starts the attempt, so a session holder cannot choose them **(call)**.

The actions are **Yes, continue**, a prominent **No**, and a smaller **I didn't start this**.

- **Yes** needs UV and a signature over a new challenge kind, `pc_setup`, with the scanned token as
  `response`. It follows B.2 unchanged: the phone recomputes the context hash from what it shows.
  Canonical context: attempt id, `sid` hash, client description, session `created_at`, display fields.
- **No** ends the attempt.
- **I didn't start this** ends the attempt and the requesting session and records a high-severity
  event. It also applies F3's report remediation to what created that session: a password or TOTP
  behind it is marked compromised, because leaving it working lets the thief start again **(call)**. A
  session from QR sign-in or OIDC has no such Kaiku credential. The session may be an attacker's; this
  is the one place where a phone's answer ends a session it did not start, and it can only end the
  session that showed the QR. The screen warns first if the report would remove the account's only
  password sign-in.

### Step 4: the code

On approval the server generates a 6-digit code, stores its keyed hash and returns the digits to the
phone only. The phone shows:

> **Type this code on the computer in front of you.**
> **Never tell it to anyone or type it on a site someone sent you.**

The code is never shown again. If the app is closed or the response is lost, the person presses
**Start again** on the PC, which is always available, and approves anew **(call)**.

The PC, polling over its session, moves to a keypad. The code is valid for 5 minutes. Three wrong
entries end the attempt, and the phone and the PC both say so. There is no per-account counter, as in
B.5: a wrong entry can only be made by the session that holds the attempt, and each attempt needs a
new approval on the phone.

### Step 5: the passkey

The right code creates an enrolment (SM2, new variant `pc_setup`) with a new `issuer_kind` that records
both the approving key device and the requesting session. Both B.7 rule 2 cascades apply: ending that
session or revoking that device voids the enrolment at once **(call)**. The setup token is returned
once, to that session only, in the response. If that response is lost, **Start again** is the way back.

- **Browser client:** the same page runs the registration with the token kept in memory, reusing
  `SetupLink.tsx`'s logic.
- **Desktop app:** the app opens the system browser at `/setup` with the token in the URL fragment, as
  a shared setup link does today, and shows "Finish in your browser". It polls the enrolment status
  over its session.

`/setup` calls the existing `POST /auth/enrollments/passkey/start` and `…/finish`. For this variant
`finish` goes straight to `done` with no second code. The token cannot claim a key device, only
register a passkey. It lives 10 minutes, as in SM2.

At `done`, in one transaction under the B.7 lock order, the server also rechecks that the requesting
session is live and the approving key device is still active with `may_approve` on, and otherwise
moves the enrolment to `expired`. The passkey is `active`.

A failed or cancelled browser ceremony can be retried until the token expires, as today. Firefox on
Linux gets the existing `LinuxPasskeyHint` guidance before the ceremony, and the PC's screen offers
"Start again" after expiry.

### Step 6: signing in

The PC shows **Sign in with your passkey**, which runs F1 and replaces the probation session
(`replaceSessionWithPasskey`). The session's assurance is never changed in place.

### Notices and audit

The existing "new passkey" high-severity notice goes to all addresses (devices, recovery contact,
admins) with "That wasn't me", and names both the PC and the approving phone. "That wasn't me" revokes
the passkey and ends its sessions, as for F5a. Additional events, coalesced under B.5:

- `pc_setup_reported` (high severity) for "I didn't start this";
- `pc_setup_failed` for three wrong codes. It goes to the account only.

Starting an attempt, approving it and refusing it are audit-logged; the rows carry no token or code.

## State machine (normative, added to B.7 as SM5)

| From | Event | Who may trigger | To | Rules |
|---|---|---|---|---|
| – | start | live probation session | `waiting` | one live per session; token rotates every 60 s; attempt ≤10 min |
| `waiting` | scan | eligible key device of the same account | `claimed` | creates the `pc_setup` challenge, TTL 2 min; backing out returns to `waiting` |
| `claimed` | approve | that device, with UV | `code` | challenge consumed in the same transaction; code 5 min; the code goes only to the device |
| `claimed` | No | that device | `refused` | |
| `claimed` | I didn't start this | that device | `reported` | ends the requesting session |
| `code` | right code | the requesting session | `enrolled` | enrolment `pc_setup` created in the same transaction |
| `code` | third wrong code | the requesting session | `failed` | `pc_setup_failed` |
| `waiting`, `claimed`, `code` | cancel | the requesting session | `cancelled` | the phone says it was cancelled |
| `waiting`, `claimed`, `code` | timer, or the session or device is no longer valid | server | `expired` | |

After `enrolled`, SM2 applies: `issued` → (passkey finish) → `done`, or `cancelled` / `expired`.

**Timers (call):** the 10-minute bound covers `waiting` and `claimed`. The code's 5 minutes run from the
approval, and the setup token's 10 minutes from the right code, each on its own clock. The longest run
is therefore about 25 minutes.

## Changes to the login spec

To be applied with this feature, as revision 6:

- **B.2, kinds table:** add `pc_setup` as an approval (Requester: the attempt; answering device: the
  scanning key device, `may_approve` on; `response`: the QR token; TTL 2 min from the claim).
- **B.4:** probation may start this attempt and enter its code, and nothing else about it. "Create
  setup codes" stays forbidden: the setup token here exists only because a key device approved.
- **SM2:** add variant `pc_setup` (issued by: an approved attempt; code: none at `claimed`, because it
  was entered in step 4; authenticator: passkey only, `active`).
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
| Phone belongs to another account, or QR rotated or unknown | keeps waiting, or a new QR | "This code isn't valid or has just changed. Scan the code on your computer again." |
| `may_approve` turned off meanwhile | "Your phone can no longer confirm this. Ask your admin." | same refusal as F2 |
| Phone says No | "Your phone declined. Start again?" | returns to the app |
| Session ended meanwhile | signed-out screen | "That session has ended." |
| Browser never finished | "This took too long. Start again?" | nothing |
| System browser cannot be opened | shows the existing "open in your browser" guidance | nothing |
| Passkey on this browser impossible | existing `passkeyFailureText` and Linux hint | nothing |

## Constraints

- Server: `server/src/approvals/` core, no forked approval logic; additive migration; `.sqlx` updated.
- Android: reuse `ui/scan/` and the login approval screen pattern; the code display follows the
  existing setup screens' size and contrast.
- Web and desktop: reuse `SetupLink.tsx`, `CodeEntry` and `ProbationBanner`; strings in the i18n files.
- No private hostnames or IPs in the public repository.
- An auth PR. Needs the maintainer's explicit merge OK, and reviews by Codex, Flash and Fable as the
  largest late auth change of v1.0.

## Tests that must exist

Server:

- `pc_setup_needs_a_live_probation_session` and is refused from a full session;
- `scan_by_other_account_looks_like_an_unknown_token`;
- `may_approve_off_refuses_scan_and_approval`;
- `qr_photo_without_the_session_gets_nothing`: another session of the same account cannot read the
  state, enter the code or get the token;
- `code_is_returned_to_the_device_only`: no PC response and no log line contains it;
- `third_wrong_code_ends_the_attempt`, `expired_code_refused`, `used_token_refused`;
- `setup_token_appears_once`; `token_cannot_claim_a_key_device`;
- `finish_without_right_code_impossible`: no token exists before the code;
- `session_ended_before_finish_expires_the_enrolment`; `device_revoked_before_finish_expires_it`;
- `i_didnt_start_this_ends_only_the_requesting_session`;
- `report_marks_the_password_or_totp_behind_the_session_compromised`;
- `foreign_rotated_and_unknown_tokens_answer_identically`;
- `ending_the_session_or_revoking_the_device_voids_the_enrolment`;
- `start_again_ends_the_old_attempt_and_needs_a_new_approval`;
- `context_comes_from_the_session_row_not_the_request`;
- `notice_names_pc_and_phone`, `that_wasnt_me_revokes_the_passkey`;
- `pc_setup_challenge_signature_golden_vector`;
- lock-order test with a concurrent key-device removal.

Client (web and desktop): button visibility by `can_set_up_this_pc`, each row of the error table, token
absent from address bar and history after `/setup` starts. Android: context-screen wording, refusal
for a foreign account, and the code shown only after UV.

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
- `client/src/views/SetupLink.tsx`, `client/src/components/auth/ProbationBanner.tsx`,
  `server/src/approvals/enrollments/`, `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/scan/`.
