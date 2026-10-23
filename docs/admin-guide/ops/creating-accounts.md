# Creating family accounts

Kaiku lets a server admin create accounts directly, so the server never has to
open public registration. The admin picks the username and a temporary
password; the new user sets their own password the first time they sign in.

## 1. Keep registration closed

Leave the registration policy at `closed` or `invite_only` (server settings /
setup). Admin-created accounts work with every policy; `POST /auth/register`
stays rejected unless the policy is `open`.

## 2. Elevate your admin session

Creating accounts is an elevated admin action. Elevate first:

```http
POST /api/admin/elevate
Authorization: Bearer <admin access token>
Content-Type: application/json

{"reason": "create family accounts"}
```

The elevation is time-limited; elevate again if it expired.

## 3. Create the account

```http
POST /api/admin/users
Authorization: Bearer <admin access token>
Content-Type: application/json

{"username": "oma", "display_name": "Oma Erna", "temporary_password": "<temporary password>"}
```

- `username`: 3–32 characters, lowercase letters, digits and `_`.
- `display_name`: optional (`null` uses the username), at most 64 characters.
- `temporary_password`: 8–128 characters.

The response is `201 Created` with the same user object that
`GET /api/admin/users` lists. A taken username answers `409`. The admin audit
log records the action as `admin.users.create` (username and display name
only; the password is never logged).

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
- the realtime connection (WebSocket) is refused with close code `1008`.

The new password must differ from the temporary one. As soon as it is set,
`password_change_required` becomes `false` and the account works normally.
