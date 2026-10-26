# Creating family accounts

Kaiku lets a server admin create accounts directly, so the server never has to
open public registration. The admin picks the username and a temporary
password; the new user sets their own password the first time they sign in.

## 1. Keep registration closed

Leave the registration policy at `closed` or `invite_only` (server settings /
setup). Admin-created accounts work with every policy; `POST /auth/register`
stays rejected unless the policy is `open`.

## 2. Elevate your admin session

Creating accounts is an elevated admin action. Elevate first by confirming your password, plus the current code from your authenticator app if MFA is on (otherwise send `null`):

```http
POST /api/admin/elevate
Authorization: Bearer <admin access token>
Content-Type: application/json

{"password": "<your password>", "mfa_code": "123456", "reason": "create family accounts"}
```

If your admin account signs in through a linked identity provider instead of a password, call `POST /auth/reauth/oidc/authorize/{provider}` with `{"redirect_uri": null, "reason": "...", "mfa_code": "<code, if MFA is on>"}`, open the returned `url` and sign in at the provider again. `GET /auth/reauth/methods` lists the options your account has. A provider you linked after this sign-in only becomes usable after you sign in again, unless you linked it while elevated.

The elevation applies only to the session you elevated, and it is time-limited. Elevate again if it has expired. Repeated wrong attempts are blocked for a while.

## 3. Create the account

```http
POST /api/admin/users
Authorization: Bearer <admin access token>
Content-Type: application/json

{"username": "oma", "display_name": "Oma Erna", "temporary_password": "<temporary password>",
 "guild_ids": ["<family server id>"]}
```

- `username`: 3–32 characters, lowercase letters, digits and `_`.
- `display_name`: optional (`null` uses the username), at most 64 characters.
- `temporary_password`: 8–128 characters.
- `guild_ids`: optional list (at most 10) of server ids the new account joins
  right away, as a normal active member (membership screening never holds an
  admin-added account back). **Add the family server here**, so the new user
  finds it as soon as they sign in. Repeated ids are ignored. If any id does
  not exist, the request fails with `400` (`VALIDATION_ERROR`, "unknown
  server") and nothing is created. More than 10 ids, or a server that has
  reached its member limit, also fail with `400` (`VALIDATION_ERROR`) and
  create nothing. Omit the field to create the account without
  any server.

The response is `201 Created` with the same user object that
`GET /api/admin/users` lists. A taken username answers `409`. The admin audit
log records the action as `admin.users.create` (username, display name and
server ids only; the password is never logged).

## 4. Hand over the temporary password

Give the username, the temporary password and the server address to the person
in person or over a channel you trust. Choose a temporary password that is
used for this one handover only.

QR login cannot be used for the very first sign-in: a QR code is created from a
device where the user is already signed in with their own password. Once the
user has finished step 5, they can add further devices (for example their
phone) by QR login.

## 5. First sign-in: the user sets their own password

After signing in with the temporary password, the login response (and
`/auth/refresh` and `GET /auth/me`) carries `password_change_required: true`.
Until the user changes the password:

- only `POST /auth/me/password`, `POST /auth/refresh`, `POST /auth/logout` and
  `GET /auth/me` work;
- every other API route answers `403` with error code
  `PASSWORD_CHANGE_REQUIRED`;
- the realtime connection (WebSocket) is refused. Clients that authenticate
  with the `authenticate` frame get an `error` event `PASSWORD_CHANGE_REQUIRED`
  and close code `1008`. Clients that send the token in the
  `Sec-WebSocket-Protocol` header get the HTTP handshake refused with `403`
  `PASSWORD_CHANGE_REQUIRED`. Clients treat both like any other auth problem:
  they show the password-change screen and do not retry blindly.

The new password must differ from the temporary one. As soon as it is set,
`password_change_required` becomes `false` and the account works normally.

**Which apps can do this:** the first-sign-in password change is built into
the desktop and Android apps. The web client does not have a forced-change
screen yet. A user who signs in there first sees errors until they change the
password in one of the supported apps. Hand the new user the desktop or
Android app for their first sign-in.
