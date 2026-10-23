# Desktop Manual Test Checklist

Manual checks for packaged desktop builds (Windows, Linux, macOS). Automated tests cannot cover
installers, OS permission prompts, keyrings, sleep/wake or real audio devices, so run this list
once per OS before a release and record the result in the table at the bottom.

## Setup

- **Server:** the dev stack, served over HTTPS by Caddy with `tls internal` on the dev host. Install the Caddy root CA on each test PC: the bundled CSP only allows `https:` and `wss:` connections.
- **Package:** download from the PR's CI artifacts: `tauri-linux`, `tauri-windows`, `tauri-macos`.
- **Server address:** set the environment variable `KAIKU_SERVER_URL=https://<dev-host>` before launching, so CI-built packages point at the dev stack (the variable is not visible to normal users).
- **Users:** create test users with `scripts/create-test-users.sh https://<dev-host>`.
- **Second participant:** a browser logged in as another test user (for DMs and voice).
- Open the devtools console where a row asks for it (right-click, Inspect, in debug/dev-enabled builds).

## Known gaps

- The Playwright e2e suite currently cannot run: `client/e2e/helpers.ts` lacks `registerAndReachMain`, a pre-existing gap.
- Entitlements on macOS apply only to signed builds. When notarising, check whether WKWebView needs additional hardened-runtime entitlements (for example JIT or unsigned executable memory) and record the outcome here.

## Windows

- [ ] Install the package from the CI artifact (SmartScreen: "More info" then "Run anyway" for unsigned builds)
- [ ] Launch the app; the login screen shows the dev stack address (the `KAIKU_SERVER_URL` override, if set)
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] Switch the microphone device while connected: audio continues on the new device
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user is forced to change the temporary password on first login (after Tasks 12-13 land)
- [ ] Log out, restart: login screen is shown

## Linux

- [ ] Install the package from the CI artifact (AppImage: `chmod +x`; or the `.deb`/`.rpm` via the package manager)
- [ ] Launch the app; the login screen shows the dev stack address (the `KAIKU_SERVER_URL` override, if set)
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] With a Secret Service keyring (GNOME Keyring or KWallet unlocked): login is remembered across restart
- [ ] Without a keyring (e.g. `dbus-run-session` with no secret service): login works for the session and a "Login not saved" toast tells you it won't be remembered; after restart the login screen is shown (not a silent broken state)
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] Switch the microphone device while connected: audio continues on the new device
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user is forced to change the temporary password on first login (after Tasks 12-13 land)
- [ ] Log out, restart: login screen is shown

## macOS

- [ ] Install the package from the CI artifact (Gatekeeper: right-click, Open, or `xattr -d com.apple.quarantine` for unsigned builds)
- [ ] Launch the app; the login screen shows the dev stack address (the `KAIKU_SERVER_URL` override, if set)
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] First voice join shows the system microphone permission prompt with the usage text; granting it works, no crash
- [ ] Denying the microphone permission shows an error, not a crash
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] Switch the microphone device while connected: audio continues on the new device
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user is forced to change the temporary password on first login (after Tasks 12-13 land)
- [ ] Log out, restart: login screen is shown

## Results

| Date | Build / commit | OS + version | Pass / fail | Notes |
|------|----------------|--------------|-------------|-------|
|      |                |              |             |       |
