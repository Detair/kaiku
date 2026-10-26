# Creating family accounts

Kaiku lets a server admin create accounts directly, so the server never has to
open public registration. The admin picks the username and a temporary
password; the new user sets their own password the first time they sign in.

## 1. Keep registration closed

Leave the registration policy at `closed` or `invite_only` (server settings /
setup), so strangers and bots cannot create accounts on their own.
Admin-created accounts work with every policy; `POST /auth/register` stays
rejected unless the policy is `open`.

## 2. Create the account in the app (primary way)

Use the desktop app (or the web client) signed in as a system admin.

1. Open **Admin Panel** (user panel at the bottom left), then **Users**, and
   click **Create account**.
2. If your session is not elevated yet, the app first shows **Unlock admin
   actions**. Confirm it is you:
   - password, plus the 6-digit **Authenticator code** if MFA is on for your
     account; or
   - **Confirm with `<provider>`** if your account signs in through a linked
     identity provider (opens the provider's login; allow pop-ups).

   The dialog only offers the methods your account has. Wrong attempts are
   rate limited. The elevation lasts 15 minutes and only applies to this
   session (the header reads "Admin Panel (Elevated)"). The dialog has no
   reason field; the optional `reason` exists only in the HTTP API.
3. Fill in the **Create account** dialog:
   - username (3-32 characters: lowercase letters, digits, `_`);
   - display name (optional);
   - temporary password (8-128 characters; **Generate new** makes one);
   - **Servers**: the new account joins every ticked server immediately as a
     normal active member. All of your servers are ticked by default, at most
     10 at once. Untick what the person should not see.
4. After creating, **Copy all** copies username, temporary password and server
   address for the handover. The password is then in your clipboard as plain
   text, and some systems sync the clipboard to the cloud or to other devices:
   copy something else afterwards.

Hand these over in person or through an end-to-end encrypted messenger, not by
ordinary e-mail, SMS or a group chat. Use the temporary password for this one
handover only.

### First sign-in: forced password change

When the new user signs in with the temporary password (web, desktop or
Android), the app shows **Choose your own password** and nothing else works
until they set one. The new password must differ from the temporary one. Then
the account works normally.

On the wire: the login response (and `/auth/refresh`, `GET /auth/me`) carries
`password_change_required: true`. Until the change, only
`POST /auth/me/password`, `POST /auth/refresh`, `POST /auth/logout` and
`GET /auth/me` work. Everything else answers `403 PASSWORD_CHANGE_REQUIRED`,
and the realtime connection is refused (WebSocket close code `1008`, or HTTP
`403` on the handshake). Apps show the password screen instead of retrying.

## 3. Log in on the phone with a QR code

A QR code is created from a device where the user is already signed in, so it
cannot replace the first sign-in with the temporary password.

1. On the desktop or web client: **Settings -> Security -> Mobile App ->
   Link Mobile Device**. The window titled "Link Mobile Device" shows the code.
2. In the Android app, on the server-address screen, tap **Scan QR code** and
   point the camera at it (camera permission is needed).
3. The app signs in as that user. No server address or password is typed.

The code is single use and valid for 120 seconds (the window counts down;
**Generate new code** makes another). It contains the server address, so the
phone must be able to reach the server at that address (public URL or VPN).
An expired or used code shows "That QR code has expired or was already used."

The code is a live one-time login, not just the server address: anyone who
sees or photographs it can sign in as that person within the 120 seconds.
Don't show it while screen sharing or to onlookers. The person always creates
the code on **their own** account. An admin never shows their own code to
someone else's phone; that phone would then be signed in with the admin's
account.

## 4. HTTP API (scripting)

Hosts and tokens below are placeholders. The API needs a system admin access
token.

How can this session confirm?

```http
GET /auth/reauth/methods
Authorization: Bearer <admin access token>
```

Elevate with password (send `"mfa_code": null` and `"reason": null` if unused):

```http
POST /api/admin/elevate
Authorization: Bearer <admin access token>
Content-Type: application/json

{"password": "<your password>", "mfa_code": "123456", "reason": "create family accounts"}
```

`200` elevates the session of this access token for 15 minutes. Errors:
`400 MFA_CODE_REQUIRED` (MFA is on, code missing), `401 INVALID_CREDENTIALS`
(wrong password or code), `400 REAUTH_OIDC_REQUIRED` (account has no password,
use a provider below), `400 REAUTH_UNAVAILABLE` (no way to confirm; ask another
admin), `400 VALIDATION_ERROR`, `429` (too many attempts; wait).

Accounts that use a provider: `POST /auth/reauth/oidc/authorize/{provider_id}`
with `{"redirect_uri": null, "reason": "...", "mfa_code": "<if MFA is on>"}`
returns a `url`; open it and sign in at the provider again. Poll
`GET /api/admin/status` (`is_elevated`) for the result. A provider linked after
this sign-in works only after you sign in again, unless you linked it while
elevated. Setup: [environment variables](../configuration/environment.md)
(`OIDC_*`, `PUBLIC_URL` is required).

Create the account:

```http
POST /api/admin/users
Authorization: Bearer <admin access token>
Content-Type: application/json

{"username": "oma", "display_name": "Oma Erna", "temporary_password": "<temporary password>",
 "guild_ids": ["<family server id>"]}
```

- `display_name`: optional (`null` uses the username), at most 64 characters.
- `guild_ids`: optional, at most 10, repeated ids are ignored. An unknown id,
  more than 10 ids, or a server at its member limit answer `400
  VALIDATION_ERROR` and create nothing. Members are active immediately
  (screening never holds them back).
- Response `201` with the user object as listed by `GET /api/admin/users`; a
  taken username answers `409`. The audit log records `admin.users.create`
  (never the password).

End the elevation early (this session only; other sessions keep theirs):

```http
DELETE /api/admin/elevate
Authorization: Bearer <admin access token>
```

## Related

- [Deployment](deployment.md) and [environment variables](../configuration/environment.md)
- [Rate limiting](../configuration/rate-limiting.md)
