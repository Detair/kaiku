# Block 2 — Clean-merge Phase 1 PRs #529–#532 Runbook

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. This is a shepherding runbook, not a traditional implementation plan — no new code is authored here. Each task is a per-PR rebase-test-push-merge cycle.

**Goal:** Land the four remaining Phase 1 PRs (#529 server security, #530 web ICE buffering, #531 Tauri RTP protocol, #532 Tauri VP8 decode) on a post-Block-1 `main` with honest green CI — no `--admin` override.

**Architecture:** Four serialized shepherding tasks, one per PR. Each task: rebase the existing worktree onto `origin/main`, resolve drift conflicts (most likely `Cargo.lock` and nightly-fmt deltas), run local gates, push, wait for green CI, squash-merge, clean up the worktree.

**Tech Stack:** `git`, `gh`, `cargo`, `bun`, `cargo-deny`. No new libraries introduced.

**Spec:** `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 2.

**Parallelization safe:** No. Tasks serialize by design — each PR rebases onto the `main` that includes all prior Block-2 merges. Skipping ahead would force double-rebases later.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Block 1 has merged to `main`**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git log origin/main --oneline | grep -E "ci-drift|CI drift|RUSTSEC-2026-0099" | head -3
```

Expected: at least one commit on `origin/main` referencing the Block 1 fix (e.g., `fix(infra): CI drift on main — fmt, rustls-webpki advisory, Makefile (#NNN)`). If nothing matches, **STOP** and complete Block 1 first.

- [ ] **Verify the four PRs are still open and mergeable**

```bash
for n in 529 530 531 532; do
  gh pr view $n --json number,title,state,headRefName,mergeable | python3 -c "import sys,json; d=json.load(sys.stdin); print(f\"#{d['number']} {d['state']} {d['mergeable']} {d['headRefName']}: {d['title']}\")"
done
```

Expected: four lines with `state=OPEN`. If any PR shows `MERGED` or `CLOSED`, skip that task — it was handled out-of-band.

- [ ] **Verify the four worktrees exist**

```bash
git worktree list | grep -E 'voice-server-security|web-voice-ice-buffering|tauri-voice-rtp-protocol|tauri-vp8-decode'
```

Expected: four matching paths. If any is missing, recreate via `git worktree add .claude/worktrees/<name> <branch>` before starting the corresponding task.

- [ ] **Environment for gradle (only Tauri/Android tasks touch it; harmless to set once)**

```bash
export JAVA_HOME="$HOME/.local/share/jdk/jdk-17.0.18+8"
export ANDROID_HOME="$HOME/.local/share/android-sdk"
export PATH="$JAVA_HOME/bin:$PATH"
```

---

## File Map

Block 2 authors zero files. The authoritative file list for each PR is owned by its original author. This runbook only rebases existing worktrees and resolves drift conflicts.

Expected *rebase conflict* files (per PR, approximate):

| PR | Likely drift files | Notes |
|----|--------------------|-------|
| #529 (server security) | `server/src/ws/handlers.rs`, `Cargo.lock` | #529 edits server code; both Block 1's fmt and its advisory bump are server-adjacent |
| #530 (web ICE buffering) | `Cargo.lock` only | Web client; should not interact with server fmt |
| #531 (Tauri RTP protocol) | `Cargo.lock` | Tauri client; may pull on `rustls-webpki` transitively — conflict likely trivial |
| #532 (Tauri VP8 decode) | `Cargo.lock`, possibly `client/src-tauri/src/*` | Depends on #531's protocol shape |

---

## Task 1: Merge #529 — server security

**Why first:** Security-sensitive fix for server rate-limiting, self-mute, and screen-share slot leak. Landing first reduces exposure time.

**Branch:** `fix/voice-server-security`
**Worktree:** `.claude/worktrees/voice-server-security`

- [ ] **Step 1: Enter the worktree and fetch latest**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/voice-server-security
git fetch origin
git status
```

Expected: `On branch fix/voice-server-security`, no local uncommitted changes. If uncommitted work exists, stop and ask — it was left by another session.

- [ ] **Step 2: Rebase onto origin/main**

```bash
git rebase origin/main
```

Expected: either clean rebase ("Successfully rebased") or conflicts enumerated by git.

- [ ] **Step 3 (conditional): Resolve conflicts**

If rebase halts on conflicts:

- For `Cargo.lock` conflicts: regenerate cleanly via `cargo build -p vc-server` (or just `cargo metadata > /dev/null`), then `git add Cargo.lock`.
- For `server/src/ws/handlers.rs` conflicts: apply nightly fmt to reconcile, then stage:

```bash
cargo +nightly fmt --all
git add server/src/ws/handlers.rs
```

- For any other conflict: read both sides carefully; prefer the PR's semantic change over `main`'s formatting change. Run `cargo +nightly fmt --all` after manual resolution. Escalate to the PR author if the conflict involves overlapping semantic edits.

Continue the rebase:

```bash
git rebase --continue
```

Repeat until rebase completes.

- [ ] **Step 4: Local gate — server tests + fmt + clippy + deny**

```bash
cargo +nightly fmt --all -- --check
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -5
cargo test -p vc-server 2>&1 | tail -5
cargo deny check advisories 2>&1 | tail -3
```

Expected: fmt clean, clippy green, `test result: ok`, `advisories ok`. If any fail, **STOP** — either fix in place (if it's a trivial drift issue) or re-assign the conflict resolution to the PR author.

- [ ] **Step 5: Force-push with lease**

```bash
git push --force-with-lease origin fix/voice-server-security
```

- [ ] **Step 6: Wait for CI to pass — no admin override**

```bash
gh pr checks 529 --watch
```

Expected: all checks pass. If any check fails that was not failing before Block 1, escalate — that's a real regression, not drift.

- [ ] **Step 7: Squash-merge**

```bash
gh pr merge 529 --squash
```

**Do NOT pass `--admin`.** If CI is still red, either the checks are not actually green (re-read output) or a PR-specific issue needs the author's attention.

- [ ] **Step 8: Verify merge landed on `main`**

```bash
git fetch origin
git log origin/main --oneline -3
```

Expected: newest commit is the squash of #529.

- [ ] **Step 9: Clean up**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/voice-server-security
git branch -D fix/voice-server-security
git fetch --prune
```

Remote deletion happens automatically on squash-merge; `--prune` removes the local tracking ref.

---

## Task 2: Merge #530 — web ICE buffering

**Branch:** `fix/web-voice-ice-buffering`
**Worktree:** `.claude/worktrees/web-voice-ice-buffering`

- [ ] **Step 1: Enter worktree, fetch, rebase**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/web-voice-ice-buffering
git fetch origin
git rebase origin/main
```

Expected: clean rebase. Web-client changes should not conflict with server fmt or server-side `Cargo.lock` bumps.

- [ ] **Step 2 (conditional): Resolve conflicts**

If `Cargo.lock` conflicts (unlikely for a web-only PR), regenerate via `cargo metadata > /dev/null` and stage. For any `client/src/**/*.ts` or `client/src/**/*.tsx` conflicts, read both sides; prefer the PR's semantic change.

- [ ] **Step 3: Local gate — web tests**

```bash
cd client
bun install --frozen-lockfile
bun run format -- --check
bun run test:run 2>&1 | tail -10
cd ..
```

Expected: format clean, `Test Files  X passed` reported. If any fail, stop.

- [ ] **Step 4: Push**

```bash
git push --force-with-lease origin fix/web-voice-ice-buffering
```

- [ ] **Step 5: Wait for CI**

```bash
gh pr checks 530 --watch
```

Expected: green.

- [ ] **Step 6: Squash-merge**

```bash
gh pr merge 530 --squash
```

- [ ] **Step 7: Verify + clean up**

```bash
git fetch origin
git log origin/main --oneline -3  # expect squash of #530

cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/web-voice-ice-buffering
git branch -D fix/web-voice-ice-buffering
git fetch --prune
```

---

## Task 3: Merge #531 — Tauri RTP protocol

**Why before #532:** #532's native VP8 decode path depends on #531's per-session RTP sequence/timestamp handling and the VP8 payload-type wire change.

**Branch:** `fix/tauri-voice-rtp-protocol`
**Worktree:** `.claude/worktrees/tauri-voice-rtp-protocol`

- [ ] **Step 1: Rebase**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/tauri-voice-rtp-protocol
git fetch origin
git rebase origin/main
```

- [ ] **Step 2 (conditional): Resolve conflicts**

Most likely `Cargo.lock`. Also possible: `client/src-tauri/src/voice*.rs` files if #529's server changes required matching Tauri-side adjustments in a shared protocol module — read carefully if that occurs. Rerun `cargo +nightly fmt --all` after manual resolution.

- [ ] **Step 3: Local gate — Tauri tests**

```bash
cargo +nightly fmt --all -- --check
# Tauri tests may be blocked by libspa on some Linux dev hosts (documented in
# Workstream A spec's "Out of scope" and Workstream G backlog). If so, skip the
# Tauri cargo test and rely on CI. Frontend + server tests still run:
SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings 2>&1 | tail -5
cargo test -p vc-client 2>&1 | tail -10 || true  # tolerate libspa build failure
```

Expected: fmt + clippy green. `cargo test -p vc-client` may fail to build on Linux due to `libspa` / pipewire-sys — that's a known pre-existing environmental issue (Workstream G). CI (which runs on ubuntu-24.04 with the right system libs) will test authoritatively.

- [ ] **Step 4: Push and wait**

```bash
git push --force-with-lease origin fix/tauri-voice-rtp-protocol
gh pr checks 531 --watch
```

- [ ] **Step 5: Squash-merge and clean up**

```bash
gh pr merge 531 --squash

git fetch origin
git log origin/main --oneline -3

cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/tauri-voice-rtp-protocol
git branch -D fix/tauri-voice-rtp-protocol
git fetch --prune
```

---

## Task 4: Merge #532 — Tauri VP8 decode

**Branch:** `feat/tauri-vp8-decode`
**Worktree:** `.claude/worktrees/tauri-vp8-decode`

- [ ] **Step 1: Rebase onto post-#531 main**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/tauri-vp8-decode
git fetch origin
git rebase origin/main
```

- [ ] **Step 2 (conditional): Resolve conflicts**

Because #532's content depends on #531's RTP changes, conflicts here are more likely than the other PRs — #531 just landed the per-session sequence/timestamp APIs that #532 consumes. Read both sides carefully; if the PR's VP8 decode path references an older signature of #531's now-renamed APIs, adjust the call sites (not the API definitions).

- [ ] **Step 3: Local gate — Tauri tests (same caveats as Task 3)**

```bash
cargo +nightly fmt --all -- --check
SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings 2>&1 | tail -5
cargo test -p vc-client 2>&1 | tail -10 || true
```

- [ ] **Step 4: Push and wait**

```bash
git push --force-with-lease origin feat/tauri-vp8-decode
gh pr checks 532 --watch
```

- [ ] **Step 5: Squash-merge and clean up**

```bash
gh pr merge 532 --squash

git fetch origin
git log origin/main --oneline -3

cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/tauri-vp8-decode
git branch -D feat/tauri-vp8-decode
git fetch --prune
```

---

## Final Verification

- [ ] **All four PRs merged**

```bash
for n in 529 530 531 532; do
  gh pr view $n --json number,state,mergedAt | python3 -c "import sys,json; d=json.load(sys.stdin); print(f\"#{d['number']} {d['state']} merged at {d['mergedAt']}\")"
done
```

Expected: four `MERGED` lines with recent timestamps.

- [ ] **No orphan worktrees or branches remain**

```bash
git worktree list | grep -E 'voice-server-security|web-voice-ice-buffering|tauri-voice-rtp-protocol|tauri-vp8-decode'
git branch --list 'fix/voice-server-security' 'fix/web-voice-ice-buffering' 'fix/tauri-voice-rtp-protocol' 'feat/tauri-vp8-decode'
```

Expected: both commands return no output.

- [ ] **`main` CI is green**

```bash
gh run list --branch main --limit 1 --json conclusion,headSha,createdAt | python3 -c "import sys,json; d=json.load(sys.stdin)[0]; print(f\"{d['headSha'][:7]} {d['conclusion']} at {d['createdAt']}\")"
```

Expected: `success` (not `failure`). If `failure`, a PR-specific issue slipped past local testing — investigate, open a follow-up fix PR, and do not admin-merge.

---

## Notes for the implementer

- **Do not bundle PRs.** Each task is its own rebase-test-merge cycle. Rebasing all four at once risks cascading conflicts and loses the "one PR, one green CI" signal Block 2 is trying to restore.
- **Do not rewrite PR history beyond rebase.** Squashing, amending, or reordering commits within an open PR is the author's prerogative, not the shepherd's. Force-push is only for the rebased tip.
- **If a PR's CI fails on something that wasn't failing on `main` before the rebase,** the PR itself has a real bug. Leave a comment on the PR pinging the author, skip to the next task, and revisit when the author pushes a fix.
- **`cargo test -p vc-client`** may fail to build on Linux due to missing `libspa` / pipewire-sys system libraries. This is a known environmental gap tracked in the Workstream G backlog. Rely on CI for authoritative results on Tauri PRs.
- **Admin override is forbidden** in this plan. If you feel the need to `--admin` any merge, stop — the point of Block 2 is to restore honest CI signal on `main`. Admin-merging a PR here defeats the entire Phase 2.5 purpose.
