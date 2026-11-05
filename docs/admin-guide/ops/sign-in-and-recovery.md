# Sign-in and recovery

Kaiku accounts sign in with a **key phone** (the Kaiku Android app holding a
key that only works with the phone's screen lock) or a passkey. This page
covers what a server admin does on the host: giving the first admin a key
phone, and removing devices or ending sessions when something went wrong.

## The host CLI: `kaiku-admin`

The server image ships a small command-line tool next to the server. It runs
with the server's own configuration, so whoever can run it already controls
the server; it needs no sign-in. Run it inside the server container:

```bash
docker compose exec server kaiku-admin <command>
```

| Command | What it does |
|---|---|
| `kaiku-admin enroll <username>` | Set up a key phone for the account (admins included); the phone is active at once |
| `kaiku-admin devices list <username>` | List the account's key phones with their id and state |
| `kaiku-admin devices remove <username> <device-id>` | Remove a key phone: its sessions end at once |
| `kaiku-admin sessions list <username>` | List the account's live sessions |
| `kaiku-admin sessions end <username> <session-id>` | End one session |
| `kaiku-admin sessions end <username> --all` | End every session of the account |

Exit codes: `0` done, `2` wrong command line (the usage is printed), `1`
failure (the reason is printed).

Every use is recorded in the admin audit log (actor: the system account).
Setting up and removing a key phone also posts a security notice to the
account's Kaiku chat and to every admin, so nobody is changed silently. Ended
sessions are signed out on their devices within seconds.

### Recovery commands

| Command | Effect |
|---|---|
| `kaiku-admin factor reset <username> (password \| totp)` | A throttled or compromised password or authenticator app works again; its sign-in delay is cleared. |
| `kaiku-admin throttle reset <username>` | Clears throttled states and sign-in delays of password and authenticator app. |
| `kaiku-admin may-approve <username> (on \| off)` | Turns phone confirmations for the account on or off. |
| `kaiku-admin recovery-contact <username> (<contact> \| --none)` | Names or removes the recovery contact (they still confirm it in the app). |
| `kaiku-admin admin demote <username>` | Removes the admin role; the admin's open admin requests are void. |
| `kaiku-admin request cancel <request-id>` | Cancels an open account request. |

They also work on admin accounts. Every command writes a `cli.*` audit row.
Every change also records a security event (`factor_restored` or
`throttle_reset`, `may_approve_changed`, `recovery_contact_named` /
`_changed`, `admin_role_removed`, `request_cancelled`, `sessions_ended`,
`authenticator_added` / `_removed`) and notifies the account; high-severity
ones also its recovery contact and every admin. The
[admin guide](../features/login/admin-guide.md#10-the-kaiku-admin-host-cli)
lists each command with its event and examples.

## Give the first admin a key phone

On a new server, or when the only admin lost their phone:

1. Install the Kaiku app on the phone that should become the key. The phone
   needs a screen lock (PIN, pattern or fingerprint).
2. On the host, run:

   ```bash
   docker compose exec server kaiku-admin enroll <admin-username>
   ```

   It prints a setup link and the same link as a QR code. The link is valid
   for 10 minutes and works once.
3. Open the link on the phone (scan the QR code with the phone's camera). The
   phone checks that it can hold a key and shows a 6-digit code.
4. The terminal shows `A device calling itself '<name>' wants to become
   <username>'s key.` Only continue if that is the phone in your hand, then
   type the code the phone shows (spaces are ignored).
5. After `Done`, the phone is the account's key and signs itself in with its
   screen lock.

Three wrong codes end the setup; run `enroll` again for a new link. The CLI
prints no secret other than the setup link, so do not paste that link
anywhere else.

## Remove a lost phone or end sessions

```bash
docker compose exec server kaiku-admin devices list alice
docker compose exec server kaiku-admin devices remove alice <device-id>
docker compose exec server kaiku-admin sessions end alice --all
```

Removing a key phone ends its sessions, voids its open confirmations and stops
its notifications after the last notice. The account and all admins get a
notice. If it was the account's only key, give the person a new one with
`enroll`.

## Before the upgrade to v1.0 (checklist)

The full day-1 walkthrough, with the migration's effects and which accounts
need `kaiku-admin enroll`, is in the
[admin guide](../features/login/admin-guide.md#2-day-1-upgrading-from-rc2-or-older).

1. Set `PUBLIC_URL` to the address members use; passkeys and setup links are
   bound to it.
2. Admins who sign in with a password only: turn on an authenticator app
   before the upgrade. Sessions of accounts with neither an authenticator app
   nor a key phone or passkey end at the upgrade.
3. Give the first admin a key phone with `kaiku-admin enroll <admin>` on the
   server host.
4. Name a recovery contact for every account and have them confirm it. A
   contact counts for a request only when they confirmed at least one delay
   period (`ACCOUNT_REQUEST_DELAY_HOURS`) before it started.
5. On a server with a single admin, accounts without another canceller can
   only be recovered with `kaiku-admin` on the host. Consider a second admin.

E-mail password reset is gone: a forgotten password is replaced through an
account recovery with a recovery code, or by an admin.

## After the upgrade to v1.0

- Accounts with a password (or a sign-in provider) and an authenticator app,
  but no key phone or passkey, get **Secure my account**: a delayed request
  that adds a phone or passkey. The Kaiku chat reminds them every 30 days
  until they do.

- Every session from before the upgrade is a limited session: chat, calls and
  the own profile, but not the admin area and not elevation. Give each admin a
  key phone with `kaiku-admin enroll` and sign in with it to use the admin area.
- Elevations from before the upgrade end with it.
- A refresh token copied before the upgrade is recognised as reused only from
  the second refresh after the upgrade on. If you suspect that tokens leaked,
  end the account's sessions with `kaiku-admin sessions end <username> --all`.
- The username `kaiku` is reserved for the system account. An existing account
  with that name is renamed to `kaiku_` and part of its id; the admin audit log
  records the new name (action `system.username_reserved`). Tell its owner.
