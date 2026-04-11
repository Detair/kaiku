# scap Fork Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop the `Detair/scap` fork dependency in `client/src-tauri/Cargo.toml` if upstream `CapSoftware/scap` main HEAD now compiles as a `vc-client` dependency on Linux. If not, open a narrow upstream PR with Detair's targeted Linux Frame enum fix.

**Architecture:** Investigation-first plan. Step 1 is testing whether upstream now builds — that may take 5 minutes and produce a one-line Cargo.toml change. If upstream still doesn't build, the rest of the plan is "open an upstream PR with our fix" and there's no code change to ship in this repo until upstream merges.

**Tech Stack:** Rust (Cargo), git, GitHub CLI

**Spec:** `docs/superpowers/specs/2026-04-11-security-audit-followups-design.md` (Topic 3)

**Branch:** `chore/scap-upstream-recheck` (only created if Step 2a succeeds)

---

## File structure

| File | Role |
|---|---|
| `client/src-tauri/Cargo.toml` | Modified only if upstream main builds (Step 2a) |

---

## Task 1: Test if upstream main HEAD builds as a `vc-client` dependency

**Files:**
- Modify: `client/src-tauri/Cargo.toml` (temporary edit, may be reverted)

The previous breakage (scap 0.1.0-beta.1 Frame enum restructure) only manifests when a downstream consumer tries to use the API. Standalone `cargo check` on scap won't catch it. The right test is "does `cargo check -p vc-client` succeed against upstream main HEAD on Linux."

- [ ] **Step 1: Get upstream main HEAD SHA**

Run: `gh api repos/CapSoftware/scap/branches/main --jq '.commit.sha'`

Expected: a 40-character SHA. Record it as `<sha>`.

- [ ] **Step 2: Save the current Cargo.toml line for later restore**

Run: `grep -n "^scap = " client/src-tauri/Cargo.toml`

Expected: a single line near line 62, currently:
```
scap = { git = "https://github.com/Detair/scap.git", branch = "fix/linux-frame-enum" }
```

Record this line — you may need to restore it if Step 2b applies.

- [ ] **Step 3: Apply temporary edit pointing at upstream main**

Edit `client/src-tauri/Cargo.toml`. Replace the existing `scap = { git = "https://github.com/Detair/scap.git", branch = "fix/linux-frame-enum" }` line with:

```toml
scap = { git = "https://github.com/CapSoftware/scap.git", rev = "<sha>" }
```

(Substitute the actual SHA from Step 1.)

**Do not commit yet.** This is a test edit.

- [ ] **Step 4: Try to build vc-client**

Run: `cargo check -p vc-client 2>&1 | tail -20`

This will pull the new scap source from GitHub, then try to type-check the Tauri client against it. The relevant target is Linux (which is where the original break happened). Build expectations:

- The first run will be slow (downloading + compiling scap and its deps). Be patient.
- The compile happens against the host target by default — usually Linux on a Linux dev machine. That's exactly what we want to test.

Expected outcomes:

a) **`cargo check` succeeds.** Upstream main now builds. Proceed to Step 2a.

b) **`cargo check` fails with a Frame enum / VideoFrame / pipewire / linux/mod.rs error.** The Linux issue is still not fixed upstream. Proceed to Step 2b.

c) **`cargo check` fails with an unrelated error** (e.g., missing system library, network failure cloning scap). Investigate and retry. Don't make a determination from a flaky failure.

- [ ] **Step 5a: If `cargo check` succeeded — proceed to Task 2**

Leave the temporary edit in place and continue to Task 2 (commit the change).

- [ ] **Step 5b: If `cargo check` failed — revert and proceed to Task 3**

Run: edit `client/src-tauri/Cargo.toml` to restore the original `scap = { git = "https://github.com/Detair/scap.git", branch = "fix/linux-frame-enum" }` line.

Run: `git diff client/src-tauri/Cargo.toml`

Expected: no diff (file restored to its original state).

Then proceed to Task 3.

---

## Task 2: Commit the upstream switch (only if Task 1 Step 5a applied)

**Files:**
- Modify: `client/src-tauri/Cargo.toml`

- [ ] **Step 1: Create the branch**

Run: `git checkout -b chore/scap-upstream-recheck`

(Created off main since you started Task 1 from main.)

- [ ] **Step 2: Update the doc comment**

**Substitute the actual `<sha>` from Task 1 Step 1 in BOTH the comment and the `rev = ` line below.** Do not commit a literal `<sha>` placeholder.

The current Cargo.toml has a multi-line comment above the scap line explaining why we're using the Detair fork. With the fork dropped, that explanation is wrong. Edit the comment to explain the new state. Replace:

```toml
# Screen capture
# Pinned to Detair/scap fork because upstream scap 0.1.0-beta.1 introduced a
# Frame enum restructure (Frame::Video(VideoFrame::*)) without updating the
# Linux pipewire backend, breaking the Linux build. Our fork (commit 4d1304e9)
# adapts the Linux backend to the new enum structure plus a Windows typo fix.
#
# Upstream PR that would fix this: https://github.com/CapSoftware/scap/pull/178
# (open since 2025-10-26, broader scope: adds sequence numbers + SystemTime +
# latest-buffer grab). Once merged and released, drop this git dep and pin to
# the next scap release. Track in #TODO (or file an issue).
scap = { git = "https://github.com/CapSoftware/scap.git", rev = "<sha>" }
```

With:

```toml
# Screen capture
# Pinned to upstream main HEAD until the next scap release. The Linux pipewire
# backend regression that required our Detair/scap fork has been resolved
# upstream. When CapSoftware/scap publishes a new release on crates.io, replace
# this git dep with a normal version pin.
scap = { git = "https://github.com/CapSoftware/scap.git", rev = "<sha>" }
```

- [ ] **Step 3: Full build verification**

`cargo check` is faster but doesn't link. Do a full build to be safe:

Run: `cargo build -p vc-client 2>&1 | tail -10`

Expected: clean build, no errors. If linker errors appear (rare with Rust), investigate.

- [ ] **Step 4: Run vc-client tests if any exist**

Run: `cargo test -p vc-client 2>&1 | tail -10`

Expected: pass (or "0 tests run" if no tests defined for vc-client). Errors here indicate something deeper than the dep change — investigate.

- [ ] **Step 5: Cargo.lock check**

Run: `git diff --stat Cargo.lock`

Expected: Cargo.lock has changed (the scap dep entries now point at the new commit). Commit it along with Cargo.toml.

- [ ] **Step 6: Commit**

**Substitute the actual `<sha>` from Task 1 Step 1 in the commit message body.**

```bash
git add client/src-tauri/Cargo.toml Cargo.lock
git commit -m "chore(client): drop Detair/scap fork, pin upstream main

The Linux pipewire backend regression in scap 0.1.0-beta.1 that required
our Detair/scap fork has been resolved upstream. Switch to upstream main
HEAD until the next scap release.

Verified: cargo build -p vc-client succeeds on Linux against upstream
commit <sha>.

Refs spec docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 3)"
```

- [ ] **Step 7: Push and PR**

Run: `git push -u origin chore/scap-upstream-recheck`

```bash
gh pr create --title "chore(client): drop Detair/scap fork, pin upstream main" --body "$(cat <<'EOF'
## Summary

Drops the Detair/scap git fork and points scap at upstream CapSoftware/scap main HEAD. The Linux pipewire backend regression that motivated the fork has been resolved upstream.

## Verification

- \`cargo check -p vc-client\` succeeds on Linux against upstream main HEAD (commit <sha>)
- \`cargo build -p vc-client\` succeeds on Linux
- All 4 build targets in CI (macos-latest, macos-15-intel, ubuntu-24.04, windows-latest) should pass

## Follow-up

When CapSoftware/scap publishes a new release on crates.io, replace the git dep with a normal version pin. Tracked in: <create issue>

## Refs

- Spec: docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 3)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 8: Verify cross-platform CI**

The PR will trigger Build jobs on macos-latest, macos-15-intel, ubuntu-24.04, and windows-latest. All must pass before merging — this is the cross-platform regression check.

If macOS or Windows fails: the upstream fix may have only addressed Linux. Don't merge. Either:
- Open the upstream issue for the macOS/Windows breakage
- Restore the Detair fork temporarily (`git revert HEAD`)
- Switch the plan to Task 3 (open our own narrow upstream PR)

If all 4 build targets pass: merge.

- [ ] **Step 9: Merge**

```bash
gh pr merge --squash --delete-branch
```

---

## Task 3: Open a narrow upstream PR (only if Task 1 Step 5b applied)

**Files:**
- None in this repo. The work is in the `CapSoftware/scap` repo.

This task involves contributing back to a third-party repo. **Get explicit user authorization before opening the PR** — submitting code to an external repo as a collaborator is a different category of action than internal repo work.

- [ ] **Step 1: Confirm the diff to upstream**

Run:

```bash
gh api repos/Detair/scap/compare/main...fix/linux-frame-enum --jq '.files[] | {filename, additions, deletions}'
```

Expected: a small set of files (likely `src/capturer/engine/linux/mod.rs` and `src/capturer/engine/win/mod.rs`). Confirm the scope is narrow — if the fork has accumulated extra changes that aren't part of the targeted fix, you may need to extract just the linux/win fixes into a smaller patch.

- [ ] **Step 2: Read the patch contents**

Run: `gh api repos/Detair/scap/compare/main...fix/linux-frame-enum --jq '.files[] | .patch'`

Verify the changes are exactly the Linux Frame enum adaptation (Frame::Video wrapper) and Windows typo fix. Nothing else.

- [ ] **Step 3: Check for an existing similar upstream PR**

Run: `gh api 'repos/CapSoftware/scap/pulls?state=all' --jq '.[] | select(.title | test("linux|frame|pipewire"; "i")) | {number, title, state}'`

The known existing PR is #178 ("fix/feat: SystemTime instead of timestamp"). It's broader than our fix. There may also be others — review them.

If there's already a narrow PR with the same content as ours: don't open a duplicate. Subscribe to the existing PR and update our Cargo.toml comment to reference it instead.

If there's no narrow PR: proceed to Step 4.

- [ ] **Step 4: ASK USER FOR EXPLICIT AUTHORIZATION**

Before opening the upstream PR, surface the plan to the user:

> "Topic 3 Step 5b applies — upstream main still doesn't build for vc-client on Linux. I want to open a narrow PR against CapSoftware/scap with the Linux Frame enum fix and Windows typo fix from our Detair/scap fork. Authorize?"

Wait for explicit yes. Do not proceed without it.

- [ ] **Step 5: Open the upstream PR**

Once authorized:

1. Fork CapSoftware/scap on GitHub (if not already forked under your account).
2. Create a branch on the fork with just the targeted commits from `Detair/scap fix/linux-frame-enum`.
3. Open the PR from the fork to `CapSoftware/scap:main`.
4. PR title: "fix(linux): adapt pipewire backend to Frame::Video enum restructure"
5. PR body: explain the breakage, the fix, and reference the original commit `4d1304e9` from Detair/scap.

(Specific gh commands omitted because they depend on which fork account is used.)

- [ ] **Step 6: Update our Cargo.toml comment with the new PR reference**

Back in this repo, on a small docs branch:

```bash
git checkout -b docs/scap-upstream-pr-tracking
```

Edit `client/src-tauri/Cargo.toml` to add the new upstream PR number alongside #178 in the existing comment:

```toml
# Upstream PRs that would fix this:
#   https://github.com/CapSoftware/scap/pull/178 (broader, open since 2025-10-26)
#   https://github.com/CapSoftware/scap/pull/<our-pr-number> (narrow, opened YYYY-MM-DD)
```

Commit and PR:

```bash
git add client/src-tauri/Cargo.toml
git commit -m "docs(client): track narrow scap upstream PR alongside #178"
git push -u origin docs/scap-upstream-pr-tracking
gh pr create --title "docs(client): track narrow scap upstream PR" --body "Tracks the narrow upstream PR opened to fix the Linux pipewire backend regression in scap. Once it merges, run Topic 3 Task 1 again to switch to upstream main."
```

- [ ] **Step 7: Done (for now)**

The actual upstream merge is out of our control. When it happens, re-run Task 1 of this plan to switch to upstream.

---

## Done criteria

**Task 2 path (upstream now builds):**
- [x] `client/src-tauri/Cargo.toml` no longer references `Detair/scap`
- [x] All 4 cross-platform Build jobs pass on the PR
- [x] PR merged to main

**Task 3 path (upstream still broken):**
- [x] No code change in main branch (Detair fork still pinned)
- [x] User authorization obtained for upstream PR
- [x] Narrow upstream PR opened to CapSoftware/scap
- [x] Our Cargo.toml comment updated to reference the new PR (small docs PR)
- [x] Tracking documented for future re-check
