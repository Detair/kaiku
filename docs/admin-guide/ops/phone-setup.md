# Setting up phones for family members

From v1.0 a family member's Android phone is their Kaiku key: it signs them in with the phone's own
screen lock and confirms sign-ins on other devices. Admins set these phones up **from their own key
phone** (Settings → Admin). The desktop and web admin panels point here.

## Before you start

- **Screen lock.** The phone needs a screen lock (PIN, pattern or password). Choose the PIN together
  with the person and write it down for them. If the phone has a fingerprint reader, add a
  fingerprint as well: it is easier to use, and adding one later does not break the setup.
- **There is no Kaiku PIN.** The prompt "Confirm it's you" is the phone's own screen lock. Kaiku never
  asks for a separate code to sign in.
- A phone without a screen lock, or without a hardware key store, cannot be a Kaiku key. Setup stops in
  the first minute and says so.

## A new account (no sign-in method yet)

1. Settings → Admin → confirm with your screen lock (admin mode stays open for 15 minutes).
2. **New account**: username and display name. There is no password.
3. Open the account → **Set up a phone** → choose how you checked who you are talking to:
   - **With me in person**: the person and the phone are with you.
   - **I called them back**: end the incoming call or chat, call them back on a number you knew
     before, and recognise them.
   Add a short note (required).
4. Confirm with your screen lock. Your phone shows a setup QR code (in person) or a link you can share
   (remotely).
5. On the new phone: install Kaiku, tap **Scan setup code** (or open the link), check the server name,
   and confirm. The new phone shows a 6-digit code.
6. Type that code on your phone (remotely, let them read it to you) and confirm with your screen lock.
   A new account is **active at once**.

Each setup code needs its own confirmation with your screen lock.

## An existing account

- **Old phone still working and with them?** Turn on **May confirm sign-ins** for the account, use
  **Add a phone** on the old phone, then turn the switch off again. The new phone works at once.
- Otherwise use **Set up a phone** as above. For an existing account the new phone becomes ready only
  after **24 hours** (`ACCOUNT_REQUEST_DELAY_HOURS`). The person, their recovery contact and all admins
  get a notice and can cancel in that time. The new phone shows when it will be ready and signs in by
  itself then.
- **Recover**: if the old phone is lost, tick **Recover** and name the lost phones; they stop working
  when the new one is ready.
- An account needs a confirmed recovery contact (or you use the host CLI) before an admin can set up
  an existing account's phone; otherwise Kaiku refuses with "Name and confirm a recovery contact first,
  or use the host CLI".

## A stolen phone

Open the account → **Remove device now** for the stolen phone **before** you start a recovery. This
works at once and does not wait 24 hours. Another admin's devices can only be removed by that admin or
with the host CLI.

## May confirm sign-ins

Family accounts created by an admin start with **May confirm sign-ins** off: their phone signs them in,
but it does not confirm sign-ins on PCs and offers no "Add a phone". This protects the least
experienced users from QR-code phishing. Turn it on only when someone needs it.

## Setup links and App Links

A setup link looks like `https://chat.example.com/setup#v=1&t=…`. The `/setup` page offers **Open in
Kaiku** on a phone. If your Android build should open these links directly (App Links), build it with
the host names of your servers:

```bash
cd mobile/android
./gradlew assembleRelease -PkaikuAppLinkHosts=chat.example.com
```

The server must publish `/.well-known/assetlinks.json` (see the passkeys guide). Without the property
the app has no App Link and "Open in Kaiku" is used instead. To check a debug build:

```bash
./gradlew :app:processDebugMainManifest -PkaikuAppLinkHosts=chat.example.com
../../scripts/android_check_app_links.sh chat.example.com
```
