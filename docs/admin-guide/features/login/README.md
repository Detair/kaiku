# The v1.0 login

From v1.0 Kaiku signs people in with **passkeys** and with the **Kaiku app on an Android phone as
the key** (the phone's own screen lock unlocks it). A password is optional, and accounts set up by
an admin have none. There is no e-mail login and no e-mail password reset. Sign-ins that can be
relayed (scanning a QR code, a password plus confirmation) are **chat only**; full access comes from
a passkey or the key phone itself. Every change to how an account signs in is announced in a
read-only chat with **Kaiku**, and recovery always waits and can be cancelled.

## The documents

| Document | For | What it covers |
|---|---|---|
| [User guide](user-guide.md) | family members | key phones, signing in on a PC, chat-only versus full, phones and passkeys on every OS, notices and "That wasn't me", the recovery contact, recovery, signing out |
| [Admin guide](admin-guide.md) | operators and admins | the day-1 upgrade, configuration (`PUBLIC_URL`, Android certificates, `assetlinks.json`), phone setup and `may_approve`, admin mode, factor states, removing devices, delays and cancellers, the security log, every `kaiku-admin` command, limits, troubleshooting, release checklist |
| [Technical reference](technical-reference.md) | reviewers, client authors | assurance classes, the session check, refresh rotation, the `kaiku-sig-v1` signature format, attestation, every sign-in flow as a sequence, step-up grants, the state machines, lock order, counters, notices, residual risks, error codes |

Task-oriented pages in the operations section:

- [Setting up phones for family members](../../ops/phone-setup.md)
- [Creating family accounts](../../ops/creating-accounts.md)
- [Passkeys](../../ops/passkeys.md)
- [Sign-in and recovery: the host CLI](../../ops/sign-in-and-recovery.md)

## Words used in these documents

| Word | Meaning |
|---|---|
| Key phone (key device) | An Android phone with the Kaiku app whose hardware key is enrolled for an account; every use needs the screen lock. |
| Passkey | A WebAuthn credential bound to the server's domain, unlocked by fingerprint, face, device PIN or a security key. |
| Strong authenticator | An active key phone or an active passkey. |
| Full / chat-only (probation) session | Fixed at sign-in: full from a passkey or the key phone itself; chat-only from QR, password or OIDC plus confirmation, TOTP, and every session from before v1.0. |
| Step-up (confirmation) | A single-use, 5-minute grant for one named action, from a passkey or the key phone. |
| Admin mode (elevation) | A 15-minute window for admin work, opened by a step-up. |
| May confirm sign-ins (`may_approve`) | Whether an account's key phones confirm sign-ins on other devices; off for accounts an admin creates. |
| Account request | A delayed change to how an account signs in (recovery, "Secure my account", an admin setup of an existing account); waits 24–72 h and can be cancelled. |
| Recovery contact | Another member (never an admin) who receives important notices and may cancel requests. |
| Recovery codes | 10 single-use codes that only start an account request. |

## Sources

The design spec (revision 5) and the contracts C26–C29 live on the design branch
(`docs/superpowers/specs/2026-11-02-phone-key-login-and-simple-app-design.md`,
`docs/superpowers/plans/2026-10-22-v1-family-contracts.md`). The implementation notes with every
deviation are in [`docs/superpowers/plans/notes/`](../../../superpowers/plans/notes/)
(`2026-11-03-l1a-*`, `-lp-*`, `-l1b-*`, `-l2a-*`, `-l2b-*`). The code is the authority; these pages
follow it and mark the places where it differs from the spec.
