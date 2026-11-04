# Creating family accounts

Kaiku lets a server admin create accounts directly, so the server never has to
open public registration. Accounts created this way have no password: the
admin then sets up the member's phone as their key (or the member adds a
passkey), and the member signs in with it.

## 1. Keep registration closed

Leave the registration policy at `closed` or `invite_only` (server settings /
setup), so strangers and bots cannot create accounts on their own.
Admin-created accounts work with every policy; `POST /auth/register` stays
rejected unless the policy is `open`.

## 2. Create the account in the app

Use the desktop app (or the web client) signed in as a system admin with your
key phone or a passkey.

1. Open **Admin Panel**, then **Users**, and click **Create account**.
2. If your session is not elevated yet, confirm **Unlock admin actions** on
   your phone (or with a passkey). The elevation lasts 15 minutes and only
   applies to this session.
3. Fill in username, display name (optional), the **Servers** the account
   joins, and optionally a **recovery contact** (another member who is not an
   admin, see [sign-in and recovery](sign-in-and-recovery.md)).

The new account has no password, no forced password change, and phone
confirmations (`may_approve`) are off until you turn them on in the user's
security panel.

## 3. Set up the member's phone (admin setup)

Admin setup is started from **your own key phone** (the Android app, user
list, **Set up a phone**). Every setup code needs its own confirmation on your
phone.

1. Choose **Set up** (a new phone) or **Recovery** (the member lost their
   phone; you can name the lost phones, they are removed when the recovery
   completes).
2. Say how you know it is the member:
   - **In person**: the member is with you;
   - **Called back**: you called the member back on a number you already
     knew. Write a short note (1 to 500 characters); it is kept with the
     setup.
3. The member opens the setup link on their phone (or scans the code). Their
   phone shows a 6-digit code; type it on **your** phone.

- **New account** (never used): the phone is the member's key at once. The
  other admins get a notice ("admin setup").
- **Existing account**: the phone waits 24 hours (`ACCOUNT_REQUEST_DELAY_HOURS`)
  as an account request. The member's devices, their recovery contact and all
  admins are told and can cancel it. The new phone shows when it will work.
  The setup is refused when the account already has an open request, or when
  nobody but you could cancel it (no other admin with a key phone, no
  confirmed recovery contact, no own key of the member).

**Same-day upgrade with a working old phone:** if the member still has a
working key phone, they use **Add a phone** on it instead; that is immediate
and needs no admin.

**Remove device now:** in the user's security panel, an admin can remove a
lost or stolen phone or passkey at once (it gets one last notice). Do this
before you start a recovery when the old phone may be in the wrong hands. It
works even on the member's last key; the member then recovers through a
request. None of the immediate actions (removing devices, phone
confirmations, marking a password or authenticator as unsafe, the recovery
contact) work on another admin's account; setup, recovery and Cancel do.

## 4. HTTP API (scripting)

Hosts and tokens below are placeholders. Every call needs a system admin with
a **full** session (signed in on a key phone, or with a passkey) and, where
noted, a confirmation of the action (a step-up grant) just before.

Create the account (elevated session):

```http
POST /api/admin/users
Authorization: Bearer <admin access token>
Content-Type: application/json

{"username": "oma", "display_name": "Oma Erna", "guild_ids": ["<family server id>"],
 "recovery_contact_id": null}
```

- `temporary_password` is no longer accepted (`400 VALIDATION_ERROR`).
- `display_name`: optional (`null` uses the username), at most 64 characters.
- `guild_ids`: optional, at most 10; an unknown id, more than 10 ids, or a
  server at its member limit answer `400 VALIDATION_ERROR` and create nothing.
- `recovery_contact_id`: optional; an admin, the system account or an unknown
  id answer `400 RECOVERY_CONTACT_INVALID`.
- Response `201` with the user object as listed by `GET /api/admin/users`
  (which now carries `is_admin` and `may_approve`); a taken username answers
  `409`. The audit log records `admin.users.create`.

Admin setup, from the admin's own key phone, confirmed as
`admin.enrollment.issue` for the user id (one confirmation per code):

```http
POST /api/admin/users/<user id>/enrollments
Content-Type: application/json

{"mode": "setup", "claimant_check": {"method": "in_person", "note": "at the kitchen table"},
 "lost_device_ids": []}
```

`201 {enrollment_id, setup_token, setup_url, expires_at}`. Errors: `403
KEY_DEVICE_REQUIRED` (not from a key phone), `400 ENROLLMENT_TARGET_SELF`,
`409 ACCOUNT_REQUEST_OPEN`, `409 NO_INDEPENDENT_CANCELLER`.

The user's security panel uses (each with its confirmation):

| Call | Confirmation |
|---|---|
| `PUT /api/admin/users/<id>/may-approve {"may_approve": true}` | `admin.may_approve` / user id |
| `PUT /api/admin/users/<id>/factors {"password": "compromised", "totp": "active"}` | `admin.factor.set_state` / user id |
| `POST /api/admin/users/<id>/authenticators/<device or passkey id>/remove` | `admin.authenticator.remove` / authenticator id |
| `PUT /api/admin/users/<id>/recovery-contact {"user_id": "<contact>"}` | `admin.recovery_contact.set` / user id |

`factors`: `"compromised"` marks a password or authenticator app as unsafe;
`"active"` allows a throttled one again. A compromised one is replaced by its
owner, not allowed again (`400 VALIDATION_ERROR`). Another admin's account
answers `403 ADMIN_TARGET_IS_ADMIN`.

`GET /api/admin/users/<id>/details` shows per user: `is_admin`,
`may_approve`, `key_devices` and `passkeys` (each `{id, label, state,
ready_at, created_at, last_used_at}`), `password_state`, `totp_state`,
`recovery_codes_left`, `bootstrap_open`, `recovery_contact`, `open_request`
and `open_sessions {full, probation}`.

## Related

- [Sign-in and recovery](sign-in-and-recovery.md)
- [Deployment](deployment.md) and [environment variables](../configuration/environment.md)
- [Rate limiting](../configuration/rate-limiting.md)
