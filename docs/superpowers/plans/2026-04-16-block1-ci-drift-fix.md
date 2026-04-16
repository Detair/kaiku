# Block 1 — CI Drift Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `main`'s CI green without admin override by fixing the nightly-rustfmt docstring drift in `server/src/ws/handlers.rs:73`, patching the `rustls-webpki` RUSTSEC-2026-0099 advisory via `cargo update`, routing `Makefile` fmt targets through nightly toolchain, and documenting the nightly requirement in standards.

**Architecture:** Four surgical commits in one PR. No new code. Two fixes for the CI red (fmt + advisory), two preventive changes so contributors don't re-introduce the same drift (Makefile + docs).

**Tech Stack:** Rust stable + nightly (for rustfmt), `cargo`, `cargo-deny`, GNU Make.

**Spec:** `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 1.

**Parallelization safe:** Single PR. No cross-PR dependencies within Phase 2.5. **Block 1 is the hard prerequisite for Block 2**; Blocks 3 and 4 can proceed in parallel to Block 1's review.

---

## Pre-flight Check (BLOCKING — run before creating the worktree)

- [ ] **Confirm `nightly` rustfmt is installed**

```bash
rustup toolchain list | grep -E '^nightly'
```

Expected: a line like `nightly-x86_64-unknown-linux-gnu`. If absent, install:

```bash
rustup toolchain install nightly --profile minimal -c rustfmt
```

- [ ] **Confirm `cargo-deny` is available**

```bash
cargo deny --version
```

Expected: something like `cargo-deny 0.X.Y`. If absent, install:

```bash
cargo install cargo-deny --locked
```

- [ ] **Confirm drift still matches spec's assumptions**

```bash
cd /home/detair/GIT/detair/kaiku
cargo +nightly fmt --all -- --check 2>&1 | grep -E "^Diff in" | head -5
```

Expected: exactly one line matching `Diff in /home/.../server/src/ws/handlers.rs:73:`. If more lines appear (other files have drifted since 2026-04-16), expand Task 1's scope but otherwise proceed.

```bash
cargo deny check advisories 2>&1 | grep -E "RUSTSEC-2026-0099"
```

Expected: at least one hit referencing `rustls-webpki 0.103.10`. If RUSTSEC-2026-0099 no longer appears (upstream yanked or `Cargo.lock` already updated), skip Task 2.

---

## Worktree Setup (run once after pre-flight passes)

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git worktree add .claude/worktrees/ci-drift-fix -b fix/ci-drift-main origin/main
cd .claude/worktrees/ci-drift-fix
```

Working branch: `fix/ci-drift-main`, based on latest `origin/main`. Working directory for all tasks below: `/home/detair/GIT/detair/kaiku/.claude/worktrees/ci-drift-fix`.

---

## File Map

| Path | Action |
|------|--------|
| `server/src/ws/handlers.rs` | Modify — apply nightly rustfmt (Task 1) |
| `Cargo.lock` | Modify — `cargo update -p rustls-webpki` delta (Task 2) |
| `Makefile` | Modify — lines 139, 143 (Task 3) |
| `docs/developer-guide/development/standards.md` | Modify — add nightly-rustfmt note (Task 4) |

---

## Task 1: Apply `cargo +nightly fmt` to the repo

**Files:**
- Modify: `server/src/ws/handlers.rs:73` (and any other files the nightly formatter flags)

- [ ] **Step 1: Confirm the failing check reproduces locally**

```bash
cargo +nightly fmt --all -- --check
```

Expected: exit code 1, diff output mentioning `server/src/ws/handlers.rs:73`.

- [ ] **Step 2: Apply the formatter**

```bash
cargo +nightly fmt --all
```

Expected: no stdout output, exit code 0.

- [ ] **Step 3: Confirm the check now passes**

```bash
cargo +nightly fmt --all -- --check
```

Expected: exit code 0, no diff output.

- [ ] **Step 4: Inspect the diff**

```bash
git diff --stat
git diff server/src/ws/handlers.rs
```

Expected: exactly one file changed (`server/src/ws/handlers.rs`), delta concentrated around line 73 — a docstring rewrap. If more files show a diff, read each to confirm the change is purely cosmetic (whitespace, line wrapping in comments/docstrings) before proceeding.

- [ ] **Step 5: Run the server test suite to verify no semantic regression**

```bash
cargo test -p vc-server 2>&1 | tail -10
```

Expected: `test result: ok`. Fmt-only changes cannot regress tests, but running them is cheap insurance against accidentally-included unrelated edits.

- [ ] **Step 6: Commit**

```bash
git add server/src/ws/handlers.rs
git status --short  # confirm only this file (plus any other fmt-only files)
git commit -m "fix(infra): apply cargo +nightly fmt to handlers.rs docstring"
```

If Step 4 revealed additional fmt-touched files outside `server/src/ws/handlers.rs`, stage them in the same commit — the message still applies (the drift is one topic).

---

## Task 2: Bump `rustls-webpki` for RUSTSEC-2026-0099

**Files:**
- Modify: `Cargo.lock` (only)

- [ ] **Step 1: Confirm the advisory reproduces**

```bash
cargo deny check advisories 2>&1 | grep -E "RUSTSEC-2026-0099|rustls-webpki" | head -5
```

Expected: output referencing `RUSTSEC-2026-0099` and `rustls-webpki v0.103.10`.

- [ ] **Step 2: Run the update**

```bash
cargo update -p rustls-webpki
```

Expected: stdout like `Updating rustls-webpki v0.103.10 -> v0.103.12` (exact version ≥0.103.12).

- [ ] **Step 3: Verify the advisory is resolved**

```bash
cargo deny check advisories 2>&1 | grep -E "RUSTSEC-2026-0099"
```

Expected: no output (exit code from the prior pipe may be non-zero; that's OK). If RUSTSEC-2026-0099 still appears, the update did not reach a non-vulnerable version — investigate `Cargo.toml` pins or transitive constraints.

- [ ] **Step 4: Confirm the full advisories check now passes**

```bash
cargo deny check advisories 2>&1 | tail -5
```

Expected: `advisories ok` or equivalent green terminal line. Any other RUSTSEC hits that were already on `main` before this task remain — we are only patching `RUSTSEC-2026-0099`.

- [ ] **Step 5: Run server tests to ensure no binary-compat break**

```bash
cargo test -p vc-server 2>&1 | tail -5
```

Expected: `test result: ok`. `rustls-webpki` is a transitive dep of `rustls`; a patch bump (0.103.10 → 0.103.12) should be ABI-stable, but verification is cheap.

- [ ] **Step 6: Inspect `Cargo.lock` delta**

```bash
git diff Cargo.lock | head -40
```

Expected: delta localized to `rustls-webpki` version entries. If unrelated deps moved (e.g., `tokio`, `serde`), stop and investigate — that would signal an unintended full-dependency-graph update.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock
git commit -m "chore(infra): cargo update -p rustls-webpki for RUSTSEC-2026-0099"
```

---

## Task 3: Route `Makefile` fmt targets through nightly toolchain

**Files:**
- Modify: `Makefile:139` and `Makefile:143`

- [ ] **Step 1: Confirm current target contents**

```bash
sed -n '138,145p' Makefile
```

Expected:
```
fmt: ## Format all code
	cargo fmt --all
	cd client && bun run format

fmt-check: ## Check code formatting
	cargo fmt --all -- --check
	cd client && bun run format -- --check
```

- [ ] **Step 2: Edit the two `cargo fmt` invocations**

Use `sed`, your editor of choice, or the Edit tool. Target transformations:

- Line 139: `	cargo fmt --all` → `	cargo +nightly fmt --all`
- Line 143: `	cargo fmt --all -- --check` → `	cargo +nightly fmt --all -- --check`

(Preserve the leading TAB character — `Makefile` requires tabs for recipe lines, not spaces.)

One-liner if preferred:

```bash
sed -i -E 's|^\tcargo fmt --all$|\tcargo +nightly fmt --all|; s|^\tcargo fmt --all -- --check$|\tcargo +nightly fmt --all -- --check|' Makefile
```

- [ ] **Step 3: Verify edits**

```bash
sed -n '138,145p' Makefile
```

Expected:
```
fmt: ## Format all code
	cargo +nightly fmt --all
	cd client && bun run format

fmt-check: ## Check code formatting
	cargo +nightly fmt --all -- --check
	cd client && bun run format -- --check
```

Leading characters must be tabs — if `cat -A Makefile | sed -n '139p'` shows `^Icargo +nightly fmt --all$` then tabs are preserved.

- [ ] **Step 4: Sanity-run `make fmt-check`**

```bash
make fmt-check
```

Expected: exit code 0 — prior tasks fixed the Rust side; the client `bun run format --check` should also pass on clean main. If the client-side check fails, that's pre-existing frontend drift and out of Block 1's scope; revert only the Makefile edit and escalate.

- [ ] **Step 5: Commit**

```bash
git add Makefile
git commit -m "chore(infra): route Makefile fmt targets through nightly toolchain"
```

---

## Task 4: Document nightly rustfmt requirement in standards.md

**Files:**
- Modify: `docs/developer-guide/development/standards.md` — append a new subsection under an existing "Development" / "Rust Tooling" heading if one exists, otherwise append a new top-level section before the final `---` separator.

- [ ] **Step 1: Locate an appropriate insertion point**

```bash
grep -nE '^##|^###' docs/developer-guide/development/standards.md | head -40
```

Find a section like "Rust" / "Development Tooling" / "Formatting" if present. If none fits cleanly, append a new `## Rust Formatting` section at the end of the file.

- [ ] **Step 2: Write the note**

Insert this content (adjust heading depth to match the surrounding document):

```markdown
### Rust Formatting Requires Nightly `rustfmt`

Kaiku's `rustfmt.toml` enables unstable features (`wrap_comments`, `format_code_in_doc_comments`, `comment_width = 100`, `normalize_comments`, `format_strings`, `imports_granularity`, `group_imports`) that only activate under nightly `rustfmt`. Stable `cargo fmt` silently ignores these options — so code that passes `cargo fmt --check` on stable can still fail CI's nightly fmt job.

**Install nightly rustfmt once per machine:**

```bash
rustup toolchain install nightly --profile minimal -c rustfmt
```

**Use `make fmt` / `make fmt-check`** — both targets route through `cargo +nightly fmt --all` so local and CI stay in sync. Do not run `cargo fmt` directly without the `+nightly` selector.

If `make fmt-check` fails with `error: no such command: +nightly`, you haven't installed nightly yet.
```

- [ ] **Step 3: Confirm the Markdown lints clean**

```bash
grep -c '^```' docs/developer-guide/development/standards.md
```

Expected: an even number (every opening fence has a closing fence).

- [ ] **Step 4: Commit**

```bash
git add docs/developer-guide/development/standards.md
git commit -m "docs(infra): note nightly rustfmt requirement in standards.md"
```

---

## Final Verification (before opening PR)

- [ ] **All four checks green locally**

```bash
cargo +nightly fmt --all -- --check && echo "fmt OK"
cargo deny check advisories 2>&1 | tail -3
cargo deny check licenses 2>&1 | tail -3
SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings 2>&1 | tail -5
make fmt-check
```

Expected:
- `fmt OK`
- `advisories ok` (no new RUSTSEC hits; RUSTSEC-2026-0099 resolved)
- `licenses ok`
- Clippy: `Finished` with no error-level diagnostics
- `make fmt-check`: exit code 0

- [ ] **Commit log review**

```bash
git log --oneline origin/main..HEAD
```

Expected exactly four commits, in order:
1. `fix(infra): apply cargo +nightly fmt to handlers.rs docstring`
2. `chore(infra): cargo update -p rustls-webpki for RUSTSEC-2026-0099`
3. `chore(infra): route Makefile fmt targets through nightly toolchain`
4. `docs(infra): note nightly rustfmt requirement in standards.md`

- [ ] **Push and open PR**

```bash
git push -u origin fix/ci-drift-main
gh pr create --base main --head fix/ci-drift-main \
  --title "fix(infra): CI drift on main — fmt, rustls-webpki advisory, Makefile" \
  --body "$(cat <<'EOF'
## Summary

Block 1 of Phase 2.5 open-topics cleanup. Fixes the drift that required admin-merging #529-#534.

- Apply `cargo +nightly fmt` to `server/src/ws/handlers.rs:73` (docstring the stable formatter silently ignored).
- `cargo update -p rustls-webpki` to resolve RUSTSEC-2026-0099 (wildcard name-constraint bypass).
- Route `Makefile`'s `fmt` / `fmt-check` targets through nightly so contributors can reproduce CI locally.
- Document the nightly-rustfmt requirement in `docs/developer-guide/development/standards.md`.

Spec: `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 1.

## Test plan

- [x] `cargo +nightly fmt --all -- --check` — exit 0
- [x] `cargo deny check advisories` — RUSTSEC-2026-0099 resolved
- [x] `cargo deny check licenses` — no regression
- [x] `cargo test -p vc-server` — green
- [x] `make fmt-check` — exit 0

## Expected CI result

`Rust Lint (fmt)`, `Rust Lint (clippy)`, `License Compliance` all **green** on this PR — the three checks that have been red on `main` since Phase 1 Phase 1 PR merges. No admin override required for merge.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Record the returned PR number for follow-up.

- [ ] **Wait for CI to pass**

```bash
gh pr checks <PR_NUMBER> --watch
```

Expected: all checks pass. If `Rust Lint (fmt)` still fails, the nightly rustfmt version on CI may have moved past what was installed locally — rerun Task 1 Step 2 with the CI's nightly version (pin via `rustup toolchain install nightly-YYYY-MM-DD` matching the CI image date) and force-push.

---

## Post-merge cleanup

After the PR is squash-merged:

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/ci-drift-fix
git branch -d fix/ci-drift-main
git push origin --delete fix/ci-drift-main
git fetch --prune
```

Signal to Block 2: once this merges, the four remaining Phase 1 PRs (#529-#532) can begin rebasing onto the new `main`.

---

## Notes for the implementer

- **Task ordering is semantically independent but commit-ordered for legibility.** Tasks 1 and 2 could run in either order; they don't interact. Task 3 can also run before Task 1 if preferred. The stated order matches the PR's commit log reader expectations (fix first, deps second, tooling third, docs last).
- **If `cargo +nightly fmt` surfaces drift beyond `handlers.rs`,** include all affected files in Task 1's commit — the topic is unchanged ("apply the formatter the CI actually runs"). Do not split into per-file commits.
- **If `cargo update -p rustls-webpki` pulls unrelated version bumps into `Cargo.lock`,** stop and investigate. A patch update of one crate should not perturb the rest of the graph; if it does, a `Cargo.toml` constraint is over-broad and needs a separate spec.
- **Do not add a pre-commit hook** in this PR. The spec explicitly defers automated enforcement; re-evaluate only if drift recurs after this PR.
