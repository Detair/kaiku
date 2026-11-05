# Signing in to Kaiku: a guide for family members

This guide explains how you sign in to Kaiku from version 1.0 on. You do not need to read it all.
Pick the part you need from the list below. Words in **bold** are the words you see in the app.

Your admin is the person who runs your Kaiku server. If anything here does not work for you,
ask them. The admin side is described in the [admin guide](admin-guide.md).

- [The short version](#the-short-version)
- [What a key phone is](#what-a-key-phone-is)
- [Signing in on a PC](#signing-in-on-a-pc)
- [Chat-only and full sign-ins](#chat-only-and-full-sign-ins)
- [Setting up your phone](#setting-up-your-phone)
- [Accounts without a password](#accounts-without-a-password)
- [Passkeys on Android, Windows, macOS and Linux](#passkeys-on-android-windows-macos-and-linux)
- [Security notices and "That wasn't me"](#security-notices-and-that-wasnt-me)
- [Your recovery contact](#your-recovery-contact)
- [Lost phone, forgotten password: recovery](#lost-phone-forgotten-password-recovery)
- [Creating a new account yourself](#creating-a-new-account-yourself)
- [Signing out](#signing-out)
- [When something goes wrong](#when-something-goes-wrong)

## The short version

- Your Android phone with the Kaiku app is your **key**. It is protected by the phone's own
  screen lock (fingerprint, PIN, pattern or password). Kaiku never asks for a separate Kaiku PIN.
- On a PC you sign in by **scanning the code** on the PC screen with your phone, or with a
  **passkey**. You can also type your password, if you have one, and then confirm on your phone.
- A sign-in by scanning or by password is **chat only**: you can chat and call, but not change
  how you sign in. For everything else, sign in with a passkey, or do it in the app on your key
  phone.
- Kaiku tells you about every new sign-in and every change in a chat called **Kaiku**. If
  something was not you, tap **That wasn't me**.
- If you lose your phone, tell your admin. If someone may know its screen lock, ask them to
  **Remove device now**.

## What a key phone is

A **key phone** is an Android phone with the Kaiku app that has been set up as the key for your
account. It holds a secret key that never leaves the phone. The key only works after you unlock
it with the phone's screen lock, every single time. If someone takes your phone but does not
know your screen lock, the key is useless to them.

Your key phone can:

- sign you in on the phone itself, without a password: when Kaiku asks **Confirm it's you**, you
  unlock with your screen lock;
- confirm sign-ins on a PC or on a second phone, when you scan the code shown there (only if your
  admin allowed it, see below);
- confirm important changes, such as removing a device or opening admin mode;
- set up another phone (**Add a phone**, only if your admin allowed it).

**May confirm sign-ins.** Accounts that an admin created start with this turned off. Then your
phone still signs you in on itself, but it does not confirm sign-ins on other devices and shows no
**Add a phone**. If you scan a sign-in code, the phone says *Your account can't confirm sign-ins on
other devices. Ask your admin.* This protects you from tricks where a stranger shows you their
code. Ask your admin to turn it on if you need it.

You can have several key phones. A PC or the desktop app is never a key.

## Signing in on a PC

The sign-in page (in a browser or in the Kaiku desktop app) offers these ways. Use the first one
that works for you.

### With a passkey (full sign-in)

Click **Sign in with passkey**, or pick your passkey from the suggestions under the user-name
field (browsers only). Your computer or phone asks for your fingerprint, face, PIN or security key.
That is all. This sign-in is **full**: you can do everything.

The desktop app opens your web browser for the passkey. Finish there; the browser page then says
**You can close this page** and the app is signed in. If the page says it was sent to you by
someone, close it: a real Kaiku page only opens because *you* clicked in *your* app. See
[Passkeys](#passkeys-on-android-windows-macos-and-linux) for how to get one.

### By scanning the code with your phone (chat only)

1. On the PC, click **Sign in with your phone**. A square code appears.
2. On your key phone, open Kaiku and tap **Scan** (one tap from the chat list).
3. Point the camera at the code on the PC.
4. The phone shows what is signing in (for example "Kaiku desktop" or a browser), the server and
   the time, and asks: **Are you signing in on a PC in front of you right now?**
5. If yes: tap **Yes, sign in** and unlock with your screen lock. The PC signs in.

If you did not start this, tap **No**. If someone asked you to scan a code, or you did not expect
this screen at all, tap **I didn't start this**: nothing is signed in and your account is warned.

Good to know:

- The code changes by itself every minute. If the phone says *This code has just changed – scan
  the new one*, just scan again.
- A sign-in screen stays open for 10 minutes. After that, start again.
- Only ever scan a code on a screen **you** are sitting in front of. Never scan a code someone sent
  you, showed you in a video call or put on a website.

### With your password, then confirm on your phone (chat only)

1. Type your user name, click **Sign in with a password**, type your password and click to sign in.
2. If your account has a key phone, the PC shows a code: **Open Kaiku on your phone and scan this
   code.** Scan it as above. The phone also says *This comes after someone entered your password.*
3. If you have no key phone but use an authenticator app (a 6-digit code app), the PC asks for
   that code instead.

If the password is wrong, or your account has no password, or password sign-in is switched off
for your account, you always see the same message: *Wrong username or password, or password
sign-in is not available for this account. Use a passkey or 'Sign in with phone', or ask an
admin.* Kaiku says the same thing in every case on purpose, so a stranger cannot find out whether
an account exists or a password is right.

If you type a wrong password many times, Kaiku makes you wait before the next try, and your account
and the admins are told. Passkeys and your phone keep working while you wait.

### "Phone not at hand?" (chat only)

On the screen that asks you to scan with your phone, click **Phone not at hand?**. If your account
has an authenticator app, you can type its 6-digit code instead. If you just used a code, wait for
the next one: each code works once.

If you typed your correct password but did not finish (you closed the page, said no, or the code
ran out), your account gets the notice *Someone entered the password of your account but did not
finish signing in*. If that was you, ignore it.

### Single sign-on

If your server offers a sign-in provider (a button like **Continue with ...**), it works like a
password: after the provider, you confirm with your phone or your authenticator code, and the
sign-in is chat only.

## Chat-only and full sign-ins

Every sign-in is one of two kinds, decided when you sign in. It never changes afterwards.

| | Full | Chat only |
|---|---|---|
| How you got it | a passkey, or the key phone signing in on itself | scanning a code, a password or sign-in provider plus confirmation, or an authenticator code; also every sign-in from before version 1.0 |
| Chat, calls, your name and photo, notification settings | yes | yes |
| Read your security log | yes | yes |
| Add or remove phones and passkeys, change password, authenticator app or recovery codes | yes, after a confirmation | no |
| End your other sessions, delete your account | yes, after a confirmation | no |
| Admin area | yes (admins only) | no |

A chat-only sign-in shows the banner *Signed in for chat only. To change sign-in settings or do
admin work, sign in with a passkey.* and a small **Chat only** chip.

**How to get a full sign-in on a PC:** click **Sign in with a passkey** in the banner. This starts
a new, full sign-in that replaces the chat-only one. If you have no passkey yet, see
[Passkeys](#passkeys-on-android-windows-macos-and-linux). On your key phone, every sign-in is
full anyway, so you can always do sensitive things there instead.

**Why scanning is chat only:** a fake website could show you a real Kaiku code and get you to scan
it. If that ever happens, the stranger can at most read and write chats until you notice (Kaiku
tells you about the new sign-in) and end it. They can never change your sign-in methods or lock
you out. A passkey cannot be tricked this way, so it gives a full sign-in.

**Confirmations.** Even in a full sign-in, important changes need one more confirmation right
before you do them: a passkey on a PC, or your screen lock on your key phone. Each confirmation
covers exactly one action (for example "Remove the device Pixel 8") and is valid for 5 minutes.

## Setting up your phone

### Your first phone

Usually your admin does this with you. You need:

- an Android phone with the Kaiku app installed;
- a **screen lock** on the phone (PIN, pattern or password). If there is none, Kaiku stops at the
  start and shows **Turn on a screen lock**. Choose a PIN you will remember; if someone helps
  you, write it down together. If the phone has a fingerprint reader, add a fingerprint as well.
  Adding a fingerprint later does not break anything.

Steps:

1. The admin (or your other device) shows a **setup code** or sends you a **setup link**.
2. In Kaiku, tap **Set up this phone with a setup code** and scan it, or open the link.
3. The phone shows the server name and asks *Set up Kaiku for <your name> on <server>?* Check that
   both are right, then confirm with your screen lock.
4. The phone shows a 6-digit code. Tell it to the person setting you up (or type it on your other
   device). It is valid for 5 minutes.
5. Done. The phone signs you in by itself.

If your account is new (never used), the phone works at once. If your account already existed,
the phone shows **Almost done** with a time: it becomes active after a waiting time of at least
24 hours. You do not need to do anything; Kaiku opens by itself then. During the wait you, your
recovery contact and all admins get a notice, and any of you can stop it. This protects you if
someone tricked your admin into setting up a phone for them. If you did not expect this, call the
person who set up your phone.

A phone that cannot be a key (no screen lock, or no secure hardware for keys) is refused right at
the start: *This phone can't be a Kaiku key.*

### A second phone, or a new phone while the old one still works

On your current key phone: **Settings → Security → Add a phone**. Confirm with your screen lock. The
old phone shows a setup code (or **Share link**).

1. On the new phone, install Kaiku and scan the code with **Set up this phone with a setup code**.
2. The new phone shows a 6-digit code.
3. The old phone says *A device calling itself '<name>' wants to become your key* and asks for the
   code. Type the code **from the phone in your hand**. Never type a code someone tells you on the
   phone or in a chat.
4. Confirm with your screen lock. The new phone is your key at once.

Three wrong codes end the setup; start again. Both phones are now keys. If you no longer use the
old one, remove it there with **Sign out and remove this phone**.

If you do not see **Add a phone**, your account may not confirm sign-ins (see above). Ask your
admin: they can turn it on for a moment while you do this, or set up the new phone for you.

### A new phone when the old one is lost or broken

See [Lost phone, forgotten password](#lost-phone-forgotten-password-recovery).

## Accounts without a password

Accounts that an admin creates for you have **no password at all**. You sign in with:

- your key phone itself (screen lock),
- a passkey, or
- scanning a code with your key phone (if your account may confirm sign-ins).

**Why this is safe.** A password can be guessed, written down, sent in a fake e-mail or reused on
another site. An account without a password has nothing to guess. Your key cannot be copied off
the phone, and it needs your screen lock every time. Kaiku also never signs anyone in by e-mail,
so a hacked e-mail account cannot open your Kaiku account.

**Without your phone.** On a PC you can still sign in with a passkey, if you have one (for example
on a security key, or one synced to your computer). If you have neither your phone nor a passkey,
ask your admin. They can set up a new phone or a passkey for you (see recovery below).

You may add a password later in the security settings, but on such an account it is usually of
no use: a password always needs a second step (your phone or an authenticator app). The settings
say so when that is the case. If you already have a phone or passkey, you can also remove a
password you no longer want (**Remove password**).

## Passkeys on Android, Windows, macOS and Linux

A **passkey** is a sign-in key stored by your device or password manager and unlocked with your
fingerprint, face, device PIN or a security key. It only works on your own Kaiku server's address,
so a fake website cannot use it. A passkey sign-in is always full.

### Getting your first passkey

Adding a passkey needs a full sign-in, so the first one comes from your key phone:

- **On the key phone itself (Android app):** **Settings → Security → Passkeys → Add a passkey**,
  confirm with your screen lock. The phone's password manager (for example Google Password Manager)
  stores it. If that password manager syncs, the passkey may also appear in the same account's
  browser on your PC.
- **For a browser on your computer:** on the key phone, **Add a phone → Share link**, send the link
  to yourself and open it in the browser on the computer. The page asks *Set up a passkey for <name>
  on <server>?*. Create the passkey; the browser then shows a 6-digit code. Type that code on your
  key phone. (This needs "May confirm sign-ins"; otherwise ask your admin for a setup link.)
- **Later passkeys:** in a browser where you signed in with a passkey, **Settings → Security →
  Passkeys → Add a passkey**, confirmed with an existing passkey.

The desktop app cannot add passkeys itself; it tells you to use the browser or the Android app.
You can have up to 10 passkeys. Rename them so you know which is which; removing one asks for a
confirmation, and removing the one you are signed in with signs you out.

### Android

In the Kaiku app, **Sign in with passkey** uses the phone's own passkey manager (Android Credential
Manager). This needs a one-time setup on the server by your admin. If the app says *This server is
not set up for passkeys in the app yet*, use your key phone or ask the admin. In the phone's
browser, passkeys work like on a computer.

### Windows

The desktop app opens your web browser for the passkey. Windows offers **Windows Hello** (face,
fingerprint or PIN), a security key, or "a phone or tablet" (you scan a code with your phone,
which must have Bluetooth on).

### macOS

The desktop app opens your browser. Safari and Chrome can use Touch ID and passkeys saved in your
Apple passwords (iCloud Keychain), a security key, or a phone nearby.

### Linux

The desktop app opens your browser.

- **Chromium and Google Chrome** can use your phone as the passkey over Bluetooth, or a security
  key.
- **Firefox cannot use your phone as a passkey**, and it has no passkey store of its own. With
  Firefox, use a FIDO2 **security key** (a small USB or NFC key; its PIN is the check), or choose
  another browser for passkeys (below).

### Choosing the browser for passkeys (desktop app)

In the desktop app: **Settings → Security → Browser for passkey sign-in**. Choose **System
default**, **Chromium**, **Google Chrome**, **Firefox**, or on macOS **Safari**. Kaiku opens this
browser whenever you sign in or confirm something with a passkey.

When the browser hands the passkey back to the app, it sends it only to the app on this computer.
Some Safari versions may show a "This form is not secure" notice for this step; you can confirm
it. If a future Chrome asks whether the page may "access apps on this device", allow it for your
Kaiku server.

## Security notices and "That wasn't me"

Kaiku writes security notices into a chat named **Kaiku**. Only Kaiku writes there; you cannot
delete, mute or hide it, and nobody who gets into your account can erase it. The same events are
listed under **Settings → Security → Security log**. Notices also arrive as phone notifications,
even when other notifications are turned off. On the lock screen they show no details.

You get a notice when, for example:

- someone signs in to your account (how, from what, and how strongly);
- a phone, passkey or authenticator app is added or removed;
- your password is set, changed, removed, paused after many wrong tries, or marked unsafe;
- someone typed your correct password but did not finish, or keeps guessing;
- a request to change how you sign in starts, is ready, completes, or is cancelled;
- an admin sets up a phone for you or changes a setting of your account.

Several notices of the same kind within 10 minutes are grouped into one ("(3 times)").

Under some notices you find buttons. They only appear where they can work:

- **That wasn't me**: you did not do this. Kaiku ends the sign-in it names, removes the phone or
  passkey that signed in or was added, and marks the password or authenticator code that was used
  as unsafe. You then set a new password or set the authenticator app up again. A phone of yours
  that only *confirmed* the event (for example you scanned a code you were tricked into scanning)
  is **not** removed, because the phone itself is fine.
- **Cancel this request**: stop a request to change how you sign in.
- **Remove this device**: remove the phone or passkey named in the notice.
- **I will help <name>**: you agree to be someone's recovery contact.

These buttons ask for a confirmation (a passkey or your screen lock). If your account has no key
phone and no passkey at all, Kaiku asks instead for a fresh code from your authenticator app or one
recovery code (*Confirm with a code*).

If "That wasn't me" would also stop your own request to secure your account, Kaiku warns you
first: *This also stops your own request to secure your account.* Read it before you continue.

## Your recovery contact

A **recovery contact** is another member of your server whom you trust, for example a sibling. It
cannot be an admin.

- They get the important notices about your account in their own Kaiku chat (what happened and
  when, never your IP address or place).
- They can **cancel** a request to change how you sign in that you did not start.
- They get **no access** to your account and cannot confirm anything for you.

Choose one under **Settings → Security → Recovery contact** (it needs a full sign-in and a
confirmation). In the browser you can pick people from your friends list; in the Android app,
people you have direct messages with. Your admin can also name one for you.

The person you choose sees **I will help <name>** and must confirm it once. A contact only counts
as a protector for requests that start at least one waiting period (normally 24 hours) after they
confirmed; the settings show **Counts from <date>**. If you change your contact, the new one must
confirm again.

## Lost phone, forgotten password: recovery

### First: is the phone in danger?

If your phone was **stolen**, or someone may know its screen lock, tell your admin right away and
ask them to **Remove device now**. That works immediately. A recovery takes at least a day, which
is too slow against a thief who can unlock the phone.

If you still have another key phone or a passkey, you do not need a recovery at all: sign in with
it, add the new phone (**Add a phone**), and remove the lost one under **Settings → Security →
Your phones**.

### Recovery codes

Recovery codes are 10 codes like `ABCDE-FGHIJ-KLM`. Each works once, and only to **start** a
recovery. A recovery code never signs you in by itself.

You get them when you set up an authenticator app at registration, when a recovery finishes, or
under **Settings → Security → Recovery codes → Create new codes** (this replaces the old set).
Write them down and keep them somewhere safe at home. Your security settings show how many are
left.

Accounts without recovery codes (for example accounts without a password) recover only through an
admin.

### Starting a recovery yourself

On the sign-in page, click **I lost my phone / forgot my password** (in the Android app this opens
the page in your browser). Enter:

1. your user name,
2. one recovery code,
3. one more step: your password, a code from your authenticator app, or a fresh sign-in at your
   sign-in provider,
4. tick **I lost my phone** if your key phones are lost: they stop working when the recovery
   completes.

Then click **Start recovery**.

- The recovery code is used up as soon as it is accepted, even if the second step is then wrong.
  The page tells you how many codes you have left.
- At most 3 recoveries can be started per account within one waiting period.
- If the details are not accepted, the page does not say which part was wrong.

### The waiting time

A recovery waits **24 hours** (your admin may have set up to 72 hours). During that time:

- your devices (also the phones you named lost), your recovery contact and all admins get a
  notice, again one hour before it is ready;
- **you** (with another key phone or passkey, or with a fresh authenticator or recovery code),
  **your recovery contact** and **any admin** can cancel it. A cancel marks the password or
  authenticator code that was used as unsafe, because it may be in a stranger's hands;
- the device on which you started it can **Withdraw request** without any other effect.

Why the wait? So that a stranger who stole your recovery code and password still cannot take your
account: you or someone close to you will notice and cancel.

### Finishing

When the request is ready, open the same page **on the same browser or desktop app** where you
started it (it remembers the request). You have 24 hours to finish. Add at least one new way to
sign in:

- **Add a passkey in this browser**,
- **Set up a phone** (scan the shown setup code with the new phone, then type the phone's 6-digit
  code here),
- a **new password**, a **new authenticator app**, and a **recovery contact**.

Then click **Finish**. You get **new recovery codes**: write them down, they are shown only once.
If you named phones lost or changed your password or authenticator app, Kaiku signs out your other
sessions (your remaining key phones stay signed in). Now sign in with what you just added.

If you lost the device where you started, wait for the request to run out and start again with
another recovery code.

### Recovery through your admin

Your admin can set up a new phone for you, in person or after calling you back on a number they
already know. For an existing account this also waits at least 24 hours, with the same notices and
the same right to cancel. When your new phone shows **Almost done**, there is nothing more to do.
Your devices show the open request with *Set up by <admin>. If that is right, there is nothing to
do.*

### "Secure my account"

Accounts from before version 1.0 that have a password (or sign-in provider) and an authenticator
app, but no key phone or passkey, see **Secure my account**. It works like a recovery, but with
your password (or sign-in provider) plus an authenticator code instead of a recovery code. After
the waiting time you add a phone or passkey and get recovery codes. Kaiku reminds you about this
every 30 days until you do it.

## Creating a new account yourself

Only if your server allows registration. After you click **Create Account**, you are **not**
signed in yet. Kaiku asks **Choose how you sign in**:

- **Use a passkey** (in this browser),
- **Use the Kaiku app on your phone** (scan the shown setup code, or in the Android app the app
  becomes the key directly),
- **Use an authenticator app** (only if you chose a password).

You have **30 minutes**. If you choose nothing in that time, the new account is deleted and you
register again. The very first account on a new server (its admin) must choose a passkey or a
phone.

## Signing out

On a **PC**, **Sign out** ends that sign-in.

On your **key phone** there are two choices:

- **Sign out**: you are signed out, but the phone stays your key. Next time, Kaiku asks **Confirm
  it's you** and you are back in with your screen lock. Security notices and calls still reach the
  phone.
- **Sign out and remove this phone**: the phone stops being your key and is signed out. You confirm
  with your screen lock. Use this when you give the phone away. You cannot remove your last phone
  or passkey this way; add another one first.

To use another account on a key phone, remove the phone as your key first (**Use another
account**).

Kaiku also asks **Confirm it's you to stay signed in** on your key phone from time to time: after a
week without use, or after 90 days at the latest. A browser sign-in lasts at most 30 days, the
desktop app at most 90 days; after a week without use you sign in again.

In **Settings → Security → Where you're signed in** (Android) or **Sessions** you see every sign-in
of your account, marked **Full** or **Chat only**, and can end them (**End**, **Sign out everywhere
else**; this needs a full sign-in and a confirmation). A key phone's sign-in is shown as **Remove
device**.

## When something goes wrong

| You see | What it means | What to do |
|---|---|---|
| *Wrong username or password, or password sign-in is not available ...* | Wrong password, or no password sign-in for this account. | Use your phone or a passkey, or ask your admin. |
| *Too many tries. Try again in N minutes.* | Many wrong passwords or codes in a row. | Wait, or sign in with your phone or a passkey. |
| *This code has just changed – scan the new one* | The PC replaced its code. | Scan again. |
| *Your account can't confirm sign-ins on other devices.* | "May confirm sign-ins" is off. | Use a passkey on the PC, or ask your admin. |
| *This code belongs to another Kaiku server. Nothing was sent.* | The code is from another server. | Make sure you are on the right page. |
| *Signed in for chat only ...* | A chat-only sign-in. | Sign in with a passkey, or use your key phone. |
| *A session was ended because its key was used twice* | Kaiku saw a sign-in token used twice and ended it for safety. | Sign in again. If it happens often, tell your admin. |
| *This phone is no longer your Kaiku key ...* | The phone was removed, or its screen lock was turned off or changed so that Android deleted the key. | Ask your admin to set it up again. |
| *This passkey was removed from Kaiku.* | The passkey no longer exists on the server. | Remove it from your device and sign in another way. |
| *This passkey is ready at <time>.* | It waits for a request to complete. | Wait until then. |
| *This is your only phone or passkey. Add another one first.* | You tried to remove your last key. | Add another phone or passkey first. |
| *This setup code was already used.* | Someone else (or another phone) used it first. | Cancel and start again with a new code. |
| *This account has been suspended.* | An admin suspended the account. | Ask the admin. |
