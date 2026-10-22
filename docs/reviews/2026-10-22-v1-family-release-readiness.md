# Kaiku v1.0 Family Release — Release-Readiness Review

> 2026-10-22 · reviewed `main` @ `aa1f319f` · goal & scope:
> `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
>
> **Disclosure note:** this repository is public and the beta is live. Two
> security Blockers are described here only at the level needed for
> planning; exploit details are kept in the maintainer's private review
> archive and will be added here once the fixes are deployed.

## Method

Six read-only review lanes (security, server, desktop, Android, UI
consistency, release/ops), each run **independently by two model
families**: Claude Fable 5.1 and Gemini 4 Argon (high). Claude Opus 5.5
merged the 12 reports, re-judged severity against the family scope and
re-read the code for every Blocker. A third model family, **Kimi K3**
(OpenCode Go), then verified every Blocker against the code. A Codex
(GPT-6-Astra) pass was attempted but hit the account usage limit; it is
scheduled as a follow-up verification of the fixes.

| | Fable | Gemini | merged |
|---|---|---|---|
| Lanes 1–3 findings | — | — | 75 (34 both models, 24 Fable-only, 17 Gemini-only) |
| Lanes 4–6 findings | — | — | 83 (9 model claims refuted/downgraded) |
| **Blockers after triage** | | | **24** (Kimi: 22 confirmed, 2 coupled to D-01, 1 runtime-only) |
| Before rollout | | | ~70 |
| Post-1.0 | | | ~62 |

Each model found Blockers the other missed (Gemini alone: Android history
never loads, desktop REST without token, QR login after restart, upload path
skipping push/permissions; Fable alone: first DM never arrives live,
desktop CORS/session refresh, onboarding gap). **The multi-model setup paid
for itself — no single model would have produced this list.**

## Headline

The **server core and the web client are in good shape**; the beta works
in a browser. But the two clients the family will actually use are not:

1. **The installed desktop app has never run in native mode.** Detection
   uses `window.__TAURI__`, which Tauri 2 only injects with
   `withGlobalTauri: true` (`tauri.conf.json:13` is `false` since the first
   commit). Native voice, keyring, global PTT, tray, notifications and the
   auto-updater (Goal 2, PRs #575–#579) are dead code in shipped builds; the
   Tauri branch behind the flag is also unfinished (38 `invoke` names not
   registered). In the browser path, login fails on CORS and sessions cannot
   refresh.
2. **The Android app cannot be used daily:** message history never loads
   (response-shape mismatch hidden by a mocked test), token refresh goes to
   `http://localhost`, a forum/announcement channel empties the channel list,
   and voice join crashes on Android 14+ (no runtime mic permission).
3. **No release has ever been produced by CI**, versions are hard-coded
   (updater would loop), and there is no signed APK.
4. **A critical security Blocker on the live server** (infrastructure
   isolation, S-01) must be fixed and deployed before anything else.

Correction to the goal document: the "desktop E2EE DM send fails" gap is
real code but **unreachable** — `sendDMMessage` has no caller, plaintext DMs
work. The remaining issue is a misleading "encrypted" padlock and the E2EE
setup prompt (both handled under scope item 3).

## Blockers (24)

Agreement: **F** Fable · **G** Gemini · **K** Kimi verification.

### Security (deploy first)
| ID | Finding | Models | Status |
|---|---|---|---|
| S-01 | Critical infrastructure-isolation issue reachable by any logged-in user (privilege escalation). *Details withheld until the fix is deployed.* | F G · K✔ | verified; VPS runtime check needed |
| S-02 | No safe onboarding: `invite_only`/`closed` reject all registration, no admin create-user → maintainer must open public registration (`auth/register.rs:80-83`) | F G · K✔ | verified |

### Server realtime & data
| ID | Finding | Models | Status |
|---|---|---|---|
| B-01 | WebSocket stays "connected" after its pub/sub forwarder dies → client receives nothing until restart (`ws/handlers.rs:427-484`) | F G · K✔ | verified |
| B-02 | Voice peers never removed on WS drop / ICE failure → ghost participants (`ws/handlers.rs:486-500`, `voice/sfu.rs:796-803`) | F G · K✔ | verified |
| B-03 | First DM to a user never reaches their open sessions live (no `DmCreated`, `chat/dm.rs`) | F · K✔ | verified |
| B-04 | No "messages since" catch-up API — prerequisite for Android reconnect (`chat/types.rs:254-260`) | F G · K✔ | verified (absence) |
| B-05 | File/photo messages skip push and several permission checks; message row inserted before upload (`chat/uploads.rs`) | G · K✔ | verified |
| B-06 | Nightly `backup.sh` very likely fails like #623 (Bitnami Postgres needs password in `docker exec`) | F G · K✔ | VPS check: do recent non-empty dumps exist? |

### Desktop (Windows / Linux / macOS)
| ID | Finding | Models | Status |
|---|---|---|---|
| D-01 | Native mode never activates (`withGlobalTauri:false` vs `"__TAURI__" in window`); Tauri branch unfinished (38 unregistered commands, `login` return-type mismatch) | F G · K✔ | verified |
| D-02 | Webview origin blocked by CORS in both server configs; compose doesn't forward `CORS_ALLOWED_ORIGINS`; `x-refresh-token` header not allowed | F · K✔ | verified; packaged-build login test |
| D-03 | Desktop session can't refresh or restore (refresh token withheld when `Origin` present; Lax cookie cross-site) | F · K✔ | verified |
| D-04 | No WebSocket reconnect in browser mode (web client today) — realtime silently dies after any drop | F G · K✔ | verified |
| D-05 | macOS: no `Info.plist` usage strings → first mic access kills the app | F G · K✔ | verified (absence) |
| D-06 | Linux webview likely has WebRTC/media disabled → no voice in browser-mode desktop | F · K? | runtime check (AppImage devtools) |
| U-02 | In Tauri mode most `lib/tauri/*` REST calls go to the webview origin without token | G · K◐ | becomes live when D-01 is fixed — same PR |
| U-03 | QR login encodes an empty server after any restart (`stores/auth.ts:128-141`) | G · K✔ | verified |

### Android
| ID | Finding | Models | Status |
|---|---|---|---|
| A-01 | Message history never loads: expects array, server returns `{items,has_more,next_cursor}`; ordering also inverted (`MessageApi.kt:32-49`) | G · K✔ | verified |
| A-02 | Token refresh goes to `http://localhost` (bypasses `defaultRequest`) → app dead ~15 min after login (`KaikuHttpClient.kt:159-171`) | F · K✔ | verified; logcat confirm |
| A-03 | Unknown channel types (forum/announcement) break the whole channel list (`Channel.kt:6-11`) | F G · K✔ | verified |
| A-04 | `RECORD_AUDIO` never requested at runtime → crash on Android 14+; `POST_NOTIFICATIONS` missing from manifest entirely | F G · K✔ | verified |

### Release & ops
| ID | Finding | Models | Status |
|---|---|---|---|
| R-01 | `release.yml` has never produced a release (wrong Dockerfile path, missing native deps, Windows shell bug, macOS Intel cross-build) | F G · K✔ | verified |
| R-02 | Version hard-coded `0.1.0` while `latest.json` uses the tag → updater loops forever | F G · K✔ | verified |
| R-03 | No signed, upgradable APK (no Android release job, `versionCode = 1`, silent unsigned fallback) | F G · K✔ | verified · infra-decision |
| R-04 | Restore procedure never drilled; runbook uses the failing `psql` form | F G · K✔ | verified; the drill is the check |

Downgraded to *Before rollout*: **U-01** desktop shows E2EE recovery-key
setup by default (inert until D-01 is fixed, then must be gated in the same
PR); **S-08** unauthenticated `/api/files/{*key}` serves any object (keys are
not enumerable; cheap prefix-allowlist fix — do it with S-01).

## Notable *Before rollout* items (selection)

- **Security:** outbound SSRF guard bypassable via redirects / DNS rebinding
  (webhooks, push); admin elevation without re-auth; updater signing key
  exposed to PR builds; port 8080 possibly published on the VPS
  (brute-force protection bypass via forwarded-for header — VPS check);
  `bun audit` 61 advisories (mermaid/dompurify actionable).
- **Android:** zombie WebSocket after network change (no pong tracking),
  relative image/avatar URLs never load, DM titles show UUIDs, errors never
  shown in chat/voice screens, category headers show UUIDs,
  `MODIFY_AUDIO_SETTINGS` missing (speaker/Bluetooth routing no-ops).
- **UI:** forum posts not clickable; announcement "follow" has no UI;
  Solarized themes fail the CLAUDE.md contrast targets but are offered in
  onboarding; "guild" vs "server" wording drift; dead "Edit" item on Android.
- **Ops:** no alerting that reaches the maintainer; no off-host backups;
  unpinned `bitnami/postgresql:latest`; live VPS compose diverges from repo
  (repo compose cannot boot as written).

Full lists: private archive (`consolidated-lanes1-3.md`,
`consolidated-lanes4-6.md`, `blockers-kimi.md`, 12 raw lane reports).

## Design inputs from the review

- **Desktop architecture — decided: native (Option A).** Fix detection via
  `@tauri-apps/api/core` `isTauri()` / `__TAURI_INTERNALS__` in one shared
  helper (lint-ban the `__TAURI__` literal); route REST through the TS
  `httpRequest` with the bearer token supplied by Rust (instead of
  re-implementing 38 commands); Rust owns tokens + refresh (keyring);
  native voice (cpal + webrtc-rs, RNNoise, global PTT) on all three OSes —
  the only path that does not depend on WebKitGTK WebRTC on Linux. CI check
  that every `invoke("…")` name is registered. Estimated 6–10 agent-days +
  one test pass per OS.
- **Push:** single APK, runtime transport selection — FCM default,
  UnifiedPush optional; content-free high-priority data messages →
  expedited WorkManager fetch → local notification; deep links.
  `POST_NOTIFICATIONS` manifest + runtime request.
- **i18n:** no catalog layer today; ~1,384 unique hardcoded strings in 181
  web/desktop files, Android has 1 `strings.xml` entry. Approach:
  `@solid-primitives/i18n` (MIT), flat typed dictionary, whole-sentence keys,
  `Intl.PluralRules`, keys mirrored in Android `strings.xml`; ESLint
  `no-restricted-syntax` on JSX text literals for new/touched files; migrate
  the ~300 family-facing core strings for v1.0.
- **Onboarding:** admin "Create account" (temporary password, forced change,
  auto-join family guild) + QR hand-over; registration policy stays closed.

## Infrastructure decisions for the maintainer

None of these will be done without an explicit OK.

1. **Security hotfix deploy** (S-01 + S-08) to
   `kaiku.pmind.de` — code by agents, deploy approval by the maintainer.
2. **VPS runtime checks** (read-only, list in the private archive):
   backup files/log, published ports, compose drift vs. repo.
3. **Firebase project** + `google-services.json` / server service-account
   key as CI/VPS secrets (FCM).
4. **Android release keystore** (generate offline, back up twice) + 4 CI
   secrets.
5. **Code signing:** Apple Developer Program (~99 USD/yr, notarisation) and
   Windows code signing (OV/EV certificate or Azure Trusted Signing) — or ship
   unsigned with illustrated install instructions.
6. **Alerting channel** + external uptime monitor.
7. **Off-host encrypted backups**, including `.env`, Tauri updater key and
   Android keystore.
8. **Pin stateful images** (Postgres, Valkey) and reconcile the live compose
   with the repo; bind 8080 to loopback.
9. **Restore drill** on the beta (test data only — safe to do).
10. Release plumbing: GitHub Environment for signing secrets, updater
    manifest hosting for RCs, `assetlinks.json` via Caddy, `KAIKU_VERSION`
    pin.

## Recommendation

**Keep the plan, re-sequence it around the findings.** v1.0 by Christmas is
still realistic because the server is solid and most Blockers are
well-localised. But the work is larger than the 15–25 days estimated before
the review: plan for **~35–45 agent-days**, run in **four parallel streams**
in separate worktrees (server / desktop / Android / release), each PR
reviewed by Fable plus one external model.

| When | Phase | Contents |
|---|---|---|
| **Oct 23–26** | 0 · Secure & observe | Security hotfix S-01/S-08 (deploy after OK); VPS read-only checks; maintainer builds the AppImage once and tries to log in (confirms D-01..D-03, D-06 on Linux); infra decisions 3–7 |
| **Oct 27–Nov 9** | 1 · Make the clients work | **Server:** B-01..B-05, catch-up API, `DmCreated`, admin create-user (S-02). **Desktop:** Option A native (D-01..D-05, U-01..U-03, `Info.plist`). **Android:** A-01..A-04, refresh, reconnect + catch-up, error surfacing. **Release:** R-01..R-03, version stamping, APK job; ops scripts (B-06/R-04) |
| **Nov 10–23** | 2 · Family features | FCM push (+ UnifiedPush) incl. @mentions & photos; i18n foundation + ~300 core strings; Before-rollout UI/Android items; alerting; **restore drill** |
| **Nov 24–30** | 3 · RC | `v1.0.0-rc.1` via CI on all platforms; full multi-model re-review of the changes (Fable + Gemini + Codex/Astra); fix |
| **Dec 1–7** | 4 · Household week | Maintainer's household on the RC daily; collect bugs |
| **Dec 8–14** | 5 · Release | Fixes → `v1.0.0` tag, curated changelog |
| **Dec 15–23** | 6 · Family rollout | Accounts created by maintainer, QR hand-over per person, buffer |

**Cut order if time runs short** (from the goal doc, refined): Android
chat extras (attachment upload, reply) → UnifiedPush option → i18n beyond
the framework → mention push (keep DM + photo push) → Linux *native* voice
(fall back to a documented browser for Linux voice). **Never cut:** the
security fixes, push, reconnect/catch-up, history loading, voice ghosts,
working desktop login, the release pipeline, the restore drill.

**Freeze:** no new features. Hide forum/announcement channels on Android
(unknown-type fallback) instead of building UIs for them.
