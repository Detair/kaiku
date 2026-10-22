# Release Runbook

> How a maintainer cuts a Kaiku release candidate or final release, and what
> to set up once beforehand. Describes `.github/workflows/release.yml` as it
> is built; if the two disagree, the workflow wins and this file is a bug.

A release builds the desktop clients (Linux AppImage/deb, Windows NSIS
installer, macOS Intel and Apple Silicon DMG), the signed Android APK, the
server image `ghcr.io/detair/kaiku-server:<version>` and the updater manifest
`latest.json`, then creates a **draft** GitHub release. Nothing is visible to
users until you publish the draft.

| Piece | Where it comes from |
|---|---|
| Version in every file | `scripts/release_version.py`, `scripts/bump-version.sh` |
| Missing-secret gating | `scripts/require_secrets.py` |
| Updater manifest | `scripts/generate_updater_manifest.py` |
| Windows signing overlay | `scripts/write_winsign_config.py` |
| APK check (signature, version, cert) | `scripts/android_verify_apk.sh` |
| Release notes | `scripts/generate_release_notes.py` (reads `CHANGELOG.md`) |

## 1. Versioning rules

Tags are `vX.Y.Z` (final) or `vX.Y.Z-rc.N` (release candidate). Each number is
0 to 99 and `N` is 1 to 98. `rc.99` is reserved and rejected.

The version lives in four files that must agree (five fields: `versionName`
and `versionCode` are both in `version.properties`). `bump-version.sh`
rewrites all of them, plus the workspace crate entries in `Cargo.lock`; never
edit them by hand:

- `client/src-tauri/tauri.conf.json`
- `Cargo.toml` (`[workspace.package]`)
- `client/package.json`
- `mobile/android/version.properties` (`versionName` and `versionCode`)

The Android `versionCode` is derived (contract C7):

```
versionCode = MAJOR*1_000_000 + MINOR*10_000 + PATCH*100 + rc     (final: rc slot = 99)
```

| Tag | versionCode |
|---|---|
| `v0.9.0-rc.1` | 90001 |
| `v1.0.0-rc.1` | 1000001 |
| `v1.0.0` | 1000099 |
| `v1.0.1-rc.1` | 1000101 |

A final always sorts above its own release candidates, and `1.0.1-rc.1` above
`1.0.0`. Android refuses to install an APK with a lower or equal `versionCode`
over an installed one.

**Never reuse or lower a version.** A tag that was pushed, even for a failed
run, is spent: fix forward with the next `rc.N`. Check the numbers before
tagging:

```sh
python3 scripts/release_version.py android-code 1.0.0-rc.1    # prints 1000001
python3 scripts/release_version.py check --tag v1.0.0-rc.1    # exit 0 = files agree
```

## 2. One-time maintainer setup (M1 to M5)

Agents never create secrets, Environments, variables or keystores. Do these
yourself. Until they are done, dry runs work and publish runs fail with
`::error::` naming the missing secret (never its value).

### M1. Android release keystore

Generate it offline, on a machine you trust, never in CI:

```sh
keytool -genkeypair -v -keystore kaiku-release.jks -alias kaiku \
  -keyalg RSA -keysize 4096 -validity 10000
base64 -w0 kaiku-release.jks > kaiku-release.jks.b64
```

Store four Environment secrets in `release` (see M3):

| Secret | Content |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | contents of `kaiku-release.jks.b64` |
| `ANDROID_KEYSTORE_PASSWORD` | keystore password |
| `ANDROID_KEY_ALIAS` | `kaiku` |
| `ANDROID_KEY_PASSWORD` | key password |

Then delete the `.b64` file. **Back up the keystore and both passwords: two
offline copies, one of them off-site (your choice where). Not on the
server.** Losing it is unrecoverable, see section 7.

### M2. Tauri updater key

The public key is already in `client/src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`). The matching private key signs every updater
bundle. Generate a pair only if you do not have the private key for that
public key; a new pair means every installed client trusts the old key and
**cannot auto-update** to builds signed with the new one.

```sh
cd client && bun run tauri signer generate -w ~/kaiku-updater.key
```

Set two secrets in Environment `release`:

| Secret | Content |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | contents of the private key file |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password; an empty value means "no password" and is valid |

Back it up like the keystore: two offline copies, one off-site, not on the
server.

### M3. Environment `release`

GitHub, Settings, Environments, New environment `release`:

- **Deployment branches and tags:** selected tags only, pattern `v*.*.*`.
- **Required reviewers:** yourself (optional but recommended: it gives you a
  last stop before the signing secrets are released to a run).
- Put the secrets from M1, M2 (and M5 if used) here, not at repository level.

Only publish runs use this Environment. Dry runs run without it and therefore
see no secrets at all.

### M4. GHCR package visibility

The first publish creates the package `ghcr.io/detair/kaiku-server` as
private. After that run: GitHub, your profile or org, Packages, `kaiku-server`,
Package settings, Change visibility, **Public** (so `docker pull` works for
self-hosters without a login). Link it to the repository if GitHub does not
do so automatically.

### M5. Code-signing hooks (optional)

Off by default. See section 8 before touching them.

## 3. Dry run

A dry run builds everything unsigned (no Environment, no secrets), pushes no
image, creates no release. It proves the pipeline still builds.

```sh
gh workflow run release.yml -f tag=v1.0.0-rc.1 -f dry_run=true
gh run watch
```

`dry_run` defaults to `true`. The dry run stamps the requested version onto
its own checkout, so the tag does not have to exist yet. Check:

- All jobs green: `prepare-release`, `build-tauri` (4 targets),
  `build-docker`, `build-android`. `publish-release` is skipped.
- Job summary of `build-android`: "Unsigned dry-run build".
- Log of each `build-tauri` job contains the `::notice::` lines "updater
  signing" skipped and, per OS, "ad-hoc signed" (macOS) or "unsigned"
  (Windows). They are expected.
- The `tauri-*` and `android-apk` artifacts are attached to the run (7 days).

## 4. Rehearsal (M6)

Run one full publish before the real RC, on a throwaway version, to prove
M1 to M4 are correct. Do it from a branch, not from `main`:

```sh
git switch -c rehearsal/0.9.0-rc.1 origin/main
scripts/bump-version.sh 0.9.0-rc.1
git commit -am "chore(infra): bump version to 0.9.0-rc.1"
git push origin rehearsal/0.9.0-rc.1
git tag -s v0.9.0-rc.1
git push origin v0.9.0-rc.1
```

The tag push triggers a publish run. Approve the Environment gate if you set
reviewers. Verify with the checklist in section 5, then clean up. Order
matters: the release first, then the tag, then the image.

```sh
gh release delete v0.9.0-rc.1 --yes
git push --delete origin v0.9.0-rc.1
git tag -d v0.9.0-rc.1
git push --delete origin rehearsal/0.9.0-rc.1
```

Delete the image version in the GitHub UI (Packages, `kaiku-server`, Package
versions, `0.9.0-rc.1`, Delete) or:

```sh
gh api --method DELETE /user/packages/container/kaiku-server/versions/<version-id>
# find <version-id> with:
gh api /user/packages/container/kaiku-server/versions --jq '.[] | {id, tags: .metadata.container.tags}'
```

(Use `/orgs/<org>/packages/...` if the repo belongs to an org.) The version
`0.9.0-rc.1` is spent: `1.0.0-rc.1` still sorts above it (versionCode 1000001
> 90001).

To exercise a publish without a tag push, dispatch on the tag ref:

```sh
gh workflow run release.yml --ref v0.9.0-rc.1 -f tag=v0.9.0-rc.1 -f dry_run=false
```

A publish via `workflow_dispatch` with `dry_run=false` passes the
"tag equals checked-out commit" check **only when dispatched with
`--ref <tag>`**. Dispatched from a branch it stops with "Tag ... points at
... but the run checked out ...", which is the safety working.

## 5. Cut an RC

1. Bump on a branch and open a PR:

   ```sh
   git switch -c chore/release-1.0.0-rc.1 origin/main
   scripts/bump-version.sh 1.0.0-rc.1
   git commit -am "chore(infra): bump version to 1.0.0-rc.1"
   git push -u origin HEAD
   gh pr create --fill
   ```

2. CI green, merge the PR. Note the merge commit:

   ```sh
   git fetch origin && git rev-parse origin/main
   ```

3. Dry-run it on that commit if the last dry run is old (section 3).
4. Tag the **merge commit** and push the tag; this starts the publish run:

   ```sh
   git tag -s v1.0.0-rc.1 <merge-sha>
   git push origin v1.0.0-rc.1
   ```

   The workflow refuses to publish if the version files do not match the tag
   or the tag is not the checked-out commit.
5. Approve the Environment gate (if set) and watch: `gh run watch`.

### Verification checklist (on the draft release)

```sh
gh release view v1.0.0-rc.1 --json assets,isDraft,isPrerelease --jq '.assets[].name'
mkdir -p /tmp/rc && cd /tmp/rc && gh release download v1.0.0-rc.1
sha256sum -c checksums.txt
```

- [ ] Assets present: `.deb`, `.AppImage` (+`.sig`), `*-setup.exe` (+`.sig`),
      both macOS `.dmg`, both `*-{x86_64,aarch64}.app.tar.gz` (+`.sig`),
      `Kaiku-1.0.0-rc.1.apk` (+`.sha256`), `checksums.txt`, `latest.json`.
- [ ] `sha256sum -c checksums.txt` passes.
- [ ] `latest.json` has 4 platforms:
      `jq -r '.platforms | keys[]' latest.json` prints `darwin-aarch64`,
      `darwin-x86_64`, `linux-x86_64`, `windows-x86_64`, each with a
      non-empty `signature`.
- [ ] The APK verifies. Needs the Android SDK build-tools (`ANDROID_HOME`):
      `scripts/android_verify_apk.sh Kaiku-1.0.0-rc.1.apk 1.0.0-rc.1 1000001`
      Compare the printed signer certificate SHA-256 with the one in the
      run's job summary and with your key backup record. The signer must be
      the same on every release or sideload upgrades break.
- [ ] The image is pullable:
      `docker pull ghcr.io/detair/kaiku-server:1.0.0-rc.1`, and
      `docker image inspect ... --format '{{ index .Config.Labels "org.opencontainers.image.version" }}'`
      prints `1.0.0-rc.1`. `latest` must **not** have moved.
- [ ] The APK is the right one (version and signer, above). The in-app
      update test (M7) comes after publishing, see below.

Then publish the draft (this is the point of no return for users):

```sh
gh release edit v1.0.0-rc.1 --draft=false
```

After publishing (the RC is a prerelease):

- [ ] Install the APK over the previous APK on a phone: upgrades in place, no
      uninstall.
- [ ] An older desktop build offers the update (M7). The updater endpoint is
      `releases/latest/download/latest.json`, and GitHub's "latest" skips
      drafts and pre-releases, so a published RC is still not offered to
      anyone. Test it with a build pointed at the RC's own manifest, reachable
      once the release is published:
      `https://github.com/Detair/kaiku/releases/download/v1.0.0-rc.1/latest.json`.

To check the manifest before publishing (that URL is a 404 while the release
is a draft), fetch it from the draft and serve it locally:

```sh
mkdir -p /tmp/rc-updater && cd /tmp/rc-updater
gh release download v1.0.0-rc.1 --pattern latest.json --clobber
python3 -m http.server 8099 --bind 127.0.0.1
```

Point a test build's updater endpoint at `http://127.0.0.1:8099/latest.json`.
The `url` fields inside still point at draft assets that only a signed-in
maintainer can download, so this proves the manifest is read and a version is
offered, not the full download. Do the full test after publishing.

## 6. Cut the final

1. Curate `CHANGELOG.md`: rename the contents of `[Unreleased]` into a
   `## [1.0.0] - YYYY-MM-DD` section (heading exactly this shape; the release
   notes script extracts the section by `[1.0.0]`) and leave a fresh empty
   `## [Unreleased]` above it. Keep a Changelog groups, user perspective.
2. Run `python3 scripts/generate_release_notes.py --version 1.0.0 --output /tmp/notes.md`
   and read `/tmp/notes.md`.
3. Bump, PR, merge, tag, exactly as in section 5, with the final version:

   ```sh
   scripts/bump-version.sh 1.0.0
   # ... commit, PR, merge ...
   git tag -s v1.0.0 <merge-sha>
   git push origin v1.0.0
   ```

4. Same verification checklist (versionCode 1000099). Here `latest` on GHCR
   **does** move, and publishing the draft makes the in-app updater offer it.

## 7. When it fails

- **A job failed for a flaky reason** (runner, network): `gh run rerun <run-id> --failed`.
  Re-running keeps the same tag, so it only works while the tag is correct.
- **Missing secret on a publish run:** the log says which secret
  (`::error::`). Add it to Environment `release`, re-run the failed jobs.
- **Tag points at the wrong commit** (or version files do not match):
  `prepare-release` fails and nothing is published. Delete the tag
  (`git push --delete origin <tag>`; `git tag -d <tag>`) and retag the right
  commit if no release or image was published for it. If anything was
  published, the version is spent: bump to the next `rc.N`.
- **Half-published run** (image pushed, release missing): delete the draft and
  the GHCR version as in section 4, then use the next version.
- **Lost Android keystore.** No recovery: Android treats an APK signed with a
  different key as a different app, so **every relative must uninstall and
  reinstall** (losing local app data) to get any future build. Back up as in
  M1 and record the signer certificate SHA-256 printed in every run summary.
- **Lost or replaced updater key.** Installed clients only accept updater
  bundles signed by the key matching their embedded public key. With the key
  gone, nobody can be auto-updated: ship a release with the new public key in
  `tauri.conf.json` and have everyone install it manually once.
- **Windows or macOS signing step fails** after enabling a hook: set the
  variable back to anything but `true`, re-run. The build falls back to
  unsigned / ad-hoc.

## 8. Code signing: options and costs

Without signing, every family member sees an OS warning on first install of
each desktop build. The pipeline ships unsigned by default and has switches
(M5) for both platforms. Android is separate: its keystore (M1) is required
and free.

| Platform | Option | Cost | Notes |
|---|---|---|---|
| macOS | Apple Developer ID | 99 USD/yr | Sign + notarise; removes the Gatekeeper prompt |
| macOS | Unsigned (ad-hoc, `APPLE_SIGNING_IDENTITY=-`) | 0 | What ships today; needs a manual override |
| Windows | Azure Trusted Signing | ~120 USD/yr | Implemented hook; identity validation required |
| Windows | Certum open-source certificate | ~70-100 EUR/yr | Not wired up |
| Windows | SignPath Foundation | free | Open-source programme, weeks of lead time; not wired up |
| Windows | OV certificate | - | Not recommended (SmartScreen reputation still has to be built) |
| Both | Unsigned, with an install guide | 0 | Free; the warnings below |

**What the family sees when unsigned:**

- **Windows (SmartScreen):** "Windows protected your PC". Click **More info**,
  then **Run anyway**.
- **macOS Sequoia (15) and later:** the app is refused with "cannot be
  opened". Open **System Settings, Privacy & Security**, scroll to the notice
  about Kaiku and click **Open Anyway**, then confirm. (Unsigned macOS builds
  are ad-hoc signed, `APPLE_SIGNING_IDENTITY=-`, which Apple Silicon needs to
  launch at all.)
- **Android:** allow "install unknown apps" for the browser or file manager
  once; later updates install over the old app.

### How to enable a hook

Both hooks are repository **variables** plus **secrets**. After setting them,
prove them with a publish run on the rehearsal tag (section 4) before the real
RC: a dry run does not exercise signing, because dry runs get no secrets.

```sh
# dispatch on the rehearsal tag; dry_run=false so the release Environment (and signing) is used
gh workflow run release.yml --ref v0.9.0-rc.1 -f tag=v0.9.0-rc.1 -f dry_run=false
```

(That needs the tag to exist with matching version files, see section 4. Also
verify the signed result on a real Windows/macOS machine.)

**macOS (Apple Developer ID).**

1. Join the Apple Developer Program, create a "Developer ID Application"
   certificate, export it as `.p12`, `base64 -w0` it.
2. Environment `release` secrets: `APPLE_CERTIFICATE` (base64 p12),
   `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`
   (`Developer ID Application: Name (TEAMID)`), `APPLE_ID`, `APPLE_PASSWORD`
   (app-specific password for notarisation), `APPLE_TEAM_ID`.
3. Repository variable: `MACOS_SIGNING_ENABLED=true`.
4. With the variable set, a publish run with any `APPLE_*` secret missing
   fails with `::error::`. Tauri signs and notarises natively.

**Windows (Azure Trusted Signing).** The enabled path has not been exercised
by any run yet.

1. Create the Trusted Signing account and certificate profile in Azure, plus
   an app registration with permission to sign.
2. Environment `release` secrets: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`,
   `AZURE_TENANT_ID`.
3. Repository variables: `AZURE_TRUSTED_SIGNING_ENDPOINT`,
   `AZURE_TRUSTED_SIGNING_ACCOUNT`, `AZURE_TRUSTED_SIGNING_PROFILE`, and
   `WINDOWS_SIGNING_ENABLED=true`.
4. **Re-evaluate before enabling (M5):** the workflow installs
   `trusted-signing-cli` 0.11.0, which upstream has deprecated in favour of
   `artifact-signing-cli`. Check the current tool, its flags and
   `scripts/write_winsign_config.py` (which emits the `signCommand`) first.

### Recommendation

**Maintainer decision:** the family uses Windows, Linux and macOS. With a Mac
in the family, buy Apple (99 USD/yr) and sign and notarise macOS. Windows
ships **unsigned for v1.0** with the install guide above; revisit Trusted
Signing or SignPath after v1.0 if the SmartScreen prompt becomes a problem.
Linux needs nothing.
