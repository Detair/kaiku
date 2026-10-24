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
- [ ] With `KAIKU_SERVER_URL` set, launch the app and sign in; sign-in works against the dev stack and DevTools → Network shows requests to that address
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] Quit, turn the network off, start the app: "Can't reach the server" screen with retry instead of the login screen; turn the network on: logged in without typing the password
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` AND the devtools network tab shows the `get_auth_info` IPC call (`ipc://localhost/get_auth_info`); together they confirm the native runtime is detected with `withGlobalTauri: false` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] 44.1 kHz headset: set a headset microphone to 2 channels, 44100 Hz (or use one that only offers that) and join voice: the browser participant hears you clearly, at normal pitch
- [ ] Unplug mic mid-call: unplug the USB microphone or headset during a call: within about 3 s an error toast says "Microphone stopped working. Check your audio settings." (the call never just shows Connected with a dead mic); choose another microphone in Voice settings: the toast disappears and the browser participant hears you again. A Bluetooth headset that takes a few seconds to switch profiles on join must NOT show the toast
- [ ] Switch mic mid-call: switch the microphone device while connected: audio continues on the new device without leaving the call; choosing a device that cannot be opened keeps the previous microphone working
- [ ] Per-person volume: there is no volume slider yet; in the devtools console run `await window.__TAURI_INTERNALS__.invoke('set_user_volume', { userId: '<browser participant user id>', volume: 0 })`: the participant goes silent at once; `volume: 0.5` is quieter, `volume: 1` restores it
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] Close the window during a voice call: one notification says Kaiku is still in the voice channel; the tray menu shows "Leave voice" and "Mute"/"Unmute"; "Leave voice" ends the call for the browser participant
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user signs in with the temporary password and sees "Choose your own password" (no "Session Expired" dialog, no "Reconnecting…"); quit and restart before changing it: the same screen appears again and asks for the temporary password; after saving a new password the app loads; signing in with the temporary password then fails
- [ ] Log out, restart: login screen is shown

## Linux

- [ ] Install the package from the CI artifact (AppImage: `chmod +x`; or the `.deb`/`.rpm` via the package manager)
- [ ] With `KAIKU_SERVER_URL` set, launch the app and sign in; sign-in works against the dev stack and DevTools → Network shows requests to that address
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] Quit, turn the network off, start the app: "Can't reach the server" screen with retry instead of the login screen; turn the network on: logged in without typing the password
- [ ] With a Secret Service keyring (GNOME Keyring or KWallet unlocked): login is remembered across restart
- [ ] Without a keyring (e.g. `dbus-run-session` with no secret service): login works for the session and a "Login not saved" toast tells you it won't be remembered; after restart the login screen is shown (not a silent broken state)
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` AND the devtools network tab shows the `get_auth_info` IPC call (`ipc://localhost/get_auth_info`); together they confirm the native runtime is detected with `withGlobalTauri: false` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] 44.1 kHz headset: set a headset microphone to 2 channels, 44100 Hz (or use one that only offers that) and join voice: the browser participant hears you clearly, at normal pitch
- [ ] Unplug mic mid-call: unplug the USB microphone or headset during a call: within about 3 s an error toast says "Microphone stopped working. Check your audio settings." (the call never just shows Connected with a dead mic); choose another microphone in Voice settings: the toast disappears and the browser participant hears you again. A Bluetooth headset that takes a few seconds to switch profiles on join must NOT show the toast
- [ ] Switch mic mid-call: switch the microphone device while connected: audio continues on the new device without leaving the call; choosing a device that cannot be opened keeps the previous microphone working
- [ ] Per-person volume: there is no volume slider yet; in the devtools console run `await window.__TAURI_INTERNALS__.invoke('set_user_volume', { userId: '<browser participant user id>', volume: 0 })`: the participant goes silent at once; `volume: 0.5` is quieter, `volume: 1` restores it
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] Close the window during a voice call: one notification says Kaiku is still in the voice channel; the tray menu shows "Leave voice" and "Mute"/"Unmute"; "Leave voice" ends the call for the browser participant
- [ ] GNOME without an AppIndicator extension (no tray icon): closing the window outside a call quits Kaiku; closing it during a call minimises it to the dock/taskbar instead of hiding it (the call keeps running and the window can be restored from there)
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user signs in with the temporary password and sees "Choose your own password" (no "Session Expired" dialog, no "Reconnecting…"); quit and restart before changing it: the same screen appears again and asks for the temporary password; after saving a new password the app loads; signing in with the temporary password then fails
- [ ] Log out, restart: login screen is shown

## macOS

- [ ] Install the package from the CI artifact (Gatekeeper: right-click, Open, or `xattr -d com.apple.quarantine` for unsigned builds)
- [ ] With `KAIKU_SERVER_URL` set, launch the app and sign in; sign-in works against the dev stack and DevTools → Network shows requests to that address
- [ ] Log in with a test user
- [ ] Quit completely, restart: still logged in, no login screen
- [ ] Quit, turn the network off, start the app: "Can't reach the server" screen with retry instead of the login screen; turn the network on: logged in without typing the password
- [ ] First voice join shows the system microphone permission prompt with the usage text; granting it works, no crash
- [ ] Denying the microphone permission shows an error, not a crash
- [ ] Send a DM to the browser participant and receive one back (both directions)
- [ ] A DM started by the browser participant appears in the list live, without reload
- [ ] Network off for 30 s: "Reconnecting…" indicator appears; after reconnecting, messages sent meanwhile appear
- [ ] Stop the server for 20 s: indicator appears after about 2 s and disappears after reconnect; a message sent meanwhile is visible after reopening the channel
- [ ] Sleep the machine for at least 20 min, wake it: app recovers without a new login (the native socket may need up to ~45 s to notice a dead connection; this is expected)
- [ ] Restart the server: app recovers without a new login
- [ ] Join a voice channel: devtools console shows `[VoiceAdapter] Creating Tauri adapter` AND the devtools network tab shows the `get_auth_info` IPC call (`ipc://localhost/get_auth_info`); together they confirm the native runtime is detected with `withGlobalTauri: false` (joining during a 1-2 s reconnect may show an error; known, retry works)
- [ ] Two-way audio with the browser participant
- [ ] Mute and deafen work and are reflected for the browser participant
- [ ] 44.1 kHz headset: set a headset microphone to 2 channels, 44100 Hz (or use one that only offers that) and join voice: the browser participant hears you clearly, at normal pitch
- [ ] Unplug mic mid-call: unplug the USB microphone or headset during a call: within about 3 s an error toast says "Microphone stopped working. Check your audio settings." (the call never just shows Connected with a dead mic); choose another microphone in Voice settings: the toast disappears and the browser participant hears you again. A Bluetooth headset that takes a few seconds to switch profiles on join must NOT show the toast
- [ ] Switch mic mid-call: switch the microphone device while connected: audio continues on the new device without leaving the call; choosing a device that cannot be opened keeps the previous microphone working
- [ ] Per-person volume: there is no volume slider yet; in the devtools console run `await window.__TAURI_INTERNALS__.invoke('set_user_volume', { userId: '<browser participant user id>', volume: 0 })`: the participant goes silent at once; `volume: 0.5` is quieter, `volume: 1` restores it
- [ ] Leave voice: no ghost entry remains in the browser participant's list
- [ ] Tray icon is shown; closing the window keeps the app running in the tray; tray click restores it
- [ ] Close the window during a voice call: one notification says Kaiku is still in the voice channel; the tray menu shows "Leave voice" and "Mute"/"Unmute"; "Leave voice" ends the call for the browser participant
- [ ] Close the window, then click the Dock icon: the window reopens
- [ ] QR code dialog shows a code that contains the server address
- [ ] DM header shows no encryption padlock; no recovery-key prompt appears after login
- [ ] Admin creates an account; the new user signs in with the temporary password and sees "Choose your own password" (no "Session Expired" dialog, no "Reconnecting…"); quit and restart before changing it: the same screen appears again and asks for the temporary password; after saving a new password the app loads; signing in with the temporary password then fails
- [ ] Log out, restart: login screen is shown

## All platforms (and the web client)

- [ ] Reconnect catch-up: user A has a channel open; cut A's network for 30 s; meanwhile B sends 3 messages and starts a brand-new DM with A; restore A's network: the 3 messages appear in order, without duplicates and without a scroll jump, and the new DM is in A's list without a reload
- [ ] Notifications while unfocused (desktop): Kaiku visible on a second screen with channel X open, focus another app; the browser participant posts in X: an OS notification appears; click into Kaiku and post again: no notification
- [ ] Push-to-talk game safety (desktop): binding plain `V` is refused with "Choose a key with Ctrl, Alt or ⌘, or F13–F24, so your game still gets the key."; bind Alt+B (or F13), focus another app: holding it transmits; in Kaiku, holding the key while typing in the message box does not transmit; an older plain binding shows one "Push-to-talk works only in Kaiku" toast and still works while Kaiku is focused
- [ ] Share a small game window (desktop, each OS): in voice, share a game or app window of about 800x600 or smaller on the "high" tier: the app does not crash, the browser participant sees the window sharp and undistorted (not stretched to 1080p); resize the window while sharing: the stream continues at the new size; share a 4K or ultrawide monitor on "medium": it arrives scaled down with the aspect ratio kept; turn on a webcam whose default resolution differs from the chosen quality: video appears, no crash

## Results

| Date | Build / commit | OS + version | Pass / fail | Notes |
|------|----------------|--------------|-------------|-------|
|      |                |              |             |       |
