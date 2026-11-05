# Passkeys

Passkeys let people sign in with their fingerprint, face, device PIN or a security key instead
of a password. A passkey sign-in is complete on its own and gives a **full** session (no phone
confirmation on top). Passkeys also confirm sensitive actions (removing a passkey or a key phone,
admin elevation): each confirmation covers one named action for up to 5 minutes.

Where passkeys work:

- **Browser:** the login page offers passkeys first (autofill in the user-name field and a
  "Sign in with passkey" button).
- **Desktop app (Windows, macOS, Linux):** the app opens the system browser (or the browser chosen
  under **Settings → Browser for passkey sign-in**) for the passkey, then takes the result back
  directly. The desktop app signs in and confirms actions with passkeys; it does not add them.
- **Android app:** through the phone's passkey manager (Credential Manager). This needs
  `assetlinks.json`, see below.

People add passkeys in **Settings → Security** in a browser or the Android app. Adding one needs a
session that is already full: an account's first passkey comes from a key phone's own session (the
phone confirms "add a passkey"), or from a setup link opened in a computer's browser (the key phone's
**Add a phone → Share link**, or an admin setup link; the browser shows a 6-digit code that is typed
on the issuing phone). Later ones come from a session signed in with a passkey.

The member-facing explanation per OS is in the
[user guide](../features/login/user-guide.md#passkeys-on-android-windows-macos-and-linux); the
protocol (profile, desktop hand-off sequence, error codes) in the
[technical reference](../features/login/technical-reference.md#75-passkey-sign-in-f1--full).

## The domain binds every passkey

The passkey "relying party ID" is the host name of `PUBLIC_URL` (for `https://chat.example.com`
it is `chat.example.com`). Browsers and phones only offer a passkey on the domain it was created
for.

> **Changing the domain makes every passkey and every key phone stop working.** People then sign
> in another way and set them up again.

The server pins the host at its first start. If `PUBLIC_URL` later names another host while
passkeys exist, the server **refuses to start** and the log names the old host, the new host and
how many passkeys are affected. Then either:

- restore `PUBLIC_URL` (a typo, a missing `www`), or
- set `PASSKEY_ALLOW_RP_ID_CHANGE=<new host>` (exactly the new host name) for one start. The
  server then revokes every passkey (their sessions end), moves the pin and starts. Remove the
  variable afterwards.

A trailing slash, upper-case letters or a non-default port in `PUBLIC_URL` do not change the
relying party ID.

## Android: `assetlinks.json`

The server publishes `https://<your domain>/.well-known/assetlinks.json` from its configuration.
It tells Android which apps may use passkeys for your domain:

- both Kaiku apps (`io.wolftown.kaiku` and `io.wolftown.kaiku.simple`, the latter is Kaiku Einfach
  from v1.2) by default (`PASSKEY_ANDROID_APP_IDS`);
- the SHA-256 of the signing certificate of the official release APKs by default
  (`PASSKEY_ANDROID_CERT_SHA256`): `A0:B9:0E:57:41:12:1A:2E:AB:DA:7C:97:5B:CC:DB:48:0F:1F:6E:05:25:FA:6A:48:D5:4F:0C:F0:AE:D3:C3:5B`.
  The official APKs from the GitHub release need no setting. Key phones do not need this file.

Requirements:

- Your reverse proxy must send `/.well-known/` to the server. The compose setup does
  (`infra/compose/Caddyfile`); for other proxies see [Web client](web-client.md#routing).
- **Google must be able to fetch the file**: no login wall, geo-block, IP allow-list or
  `robots.txt` rule in front of it. A server Google cannot reach has no passkeys in the Android
  app; the app itself is the key there.

Check it:

```bash
curl -s https://chat.example.com/.well-known/assetlinks.json
curl -s 'https://digitalassetlinks.googleapis.com/v1/statements:list?source.web.site=https://chat.example.com&relation=delegate_permission/common.get_login_creds'
```

The first shows one statement per app id; the second must list them too (Google caches the file
for a while after a change).

### Self-built APKs

Set `PASSKEY_ANDROID_CERT_SHA256` to the SHA-256 of your signing certificate (comma-separated for
several, for example release and debug). Read it with one of:

```bash
apksigner verify --print-certs kaiku.apk        # "Signer #1 certificate SHA-256 digest: …"
keytool -printcert -jarfile kaiku.apk           # "SHA256: AA:BB:…"
```

Both forms are accepted, with or without colons, in any case. Set `PASSKEY_ANDROID_APP_IDS` only
if your build uses other package names; app ids without certificates are paired with the official
certificate, which a self-built APK does not have. `none` in either variable turns passkeys in the
Android app off. The server refuses to start when one variable is `none` and the other names values.

An APK re-signed by a store (Play App Signing) has that store's certificate: add it to
`PASSKEY_ANDROID_CERT_SHA256`, together with the official one if both are in use.

## Desktop app

- The app opens the system browser, or the browser chosen under **Settings → Browser for passkey
  sign-in** (System, Chromium, Google Chrome, Firefox; Safari on macOS). The page in the browser
  names what it is for ("Sign in to the Kaiku desktop app on this computer") and says "If someone
  sent you this link, close this page."
- When the passkey is done, the browser page hands the result to the app on this computer
  (a form sent to `http://127.0.0.1:<port>`) and shows "You can close this page". The result
  only counts once the app redeems it; a link someone forwarded is useless to them.
- **Linux:** Chromium-based browsers can use a phone (over Bluetooth) as the passkey; **Firefox
  cannot**, and it has no built-in passkey store. With Firefox, register a FIDO2 security key as a
  passkey (its PIN is the verification), or choose Chromium under "Browser for passkey sign-in".
- A PC signed in by QR code is chat-only (probation) and cannot confirm anything with a passkey;
  for admin work there, sign in on that PC with a passkey (that session is full), then each
  elevation needs one passkey confirmation.
- What the browser may show on the hand-off: no current browser warns about this form or asks for
  a permission (research of 2026-11-03). Safari might show a "This form is not secure" notice in
  some versions or when "Warn before connecting to a website over HTTP" is on: confirm it, the form
  only goes to this computer. Should a future Chrome version ask to "access apps on this device",
  allow it for your Kaiku domain.

## Limits

Passkey requests have their own limit per IP address and per account (`RATE_LIMIT_AUTH_PASSKEY`,
60 per minute by default; the desktop ceremony page itself does not count). Failed passkey attempts
never count towards the failed-login IP block, so failed attempts by one person never lock others on
the same IP address out of passwords or passkeys. The per-IP limit is shared by everyone behind one
address (a household, a carrier NAT): a desktop sign-in takes about five requests, and the login
page's passkey autofill one every few minutes per open tab. Raise the limit for large groups behind
one address.

## Troubleshooting

| Error code | What it means | What to do |
|---|---|---|
| `PASSKEY_UNKNOWN` | The passkey is not (or no longer) registered on this server. | Remove it from the device ("This passkey was removed from Kaiku"), sign in another way. |
| `PASSKEY_PENDING` | The passkey belongs to an account request that is not ready yet. | Wait until the time shown, then sign in again. |
| `PASSKEY_INVALID` | Origin, verification (fingerprint/PIN) or signature check failed. | Retry; check `PUBLIC_URL` and, in the Android app, `assetlinks.json`. |
| `PASSKEY_USER_MISMATCH` | The passkey belongs to another account than the one typed or signed in. | Choose the passkey of the right account. |
| `PASSKEY_CEREMONY_INVALID` | The request expired (5 minutes) or was used already. | Start again. |
| `PASSKEY_HANDOFF_INVALID` | The desktop app could not redeem the browser's result (expired after 60 s, used, or the session changed). | Start again in the app. |
| `CLIENT_OUTDATED` | An old desktop app. | Update the app. |
| `LAST_STRONG_AUTHENTICATOR` | Removing the last passkey or key phone is refused. | Add another passkey or key phone first. |
| `PASSKEY_LIMIT` | The account already has 10 passkeys. | Remove one first. |
| `PASSKEYS_UNAVAILABLE` | Passkeys are not configured (test setups only). | Set `PUBLIC_URL`. |
| Android: "This server is not set up for passkeys in the app" | Android could not verify `assetlinks.json`. | Check the routing and the Google check above. |
