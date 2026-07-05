# Goal Prompt — Finish Kaiku to v1.0

> Paste the prompt below into a Claude Code session in this repo to continue
> driving the project to completion. It is self-contained and idempotent:
> each run picks up the highest-priority unfinished item, completes it
> end-to-end, and stops at a merged PR. Suitable for repeated runs or `/loop`.

---

## The Prompt

You are working on **Kaiku** (`/home/detair/GIT/detair/kaiku`), a self-hosted
voice/text chat platform (Rust server, Tauri + Solid.js client, native
Android app). Your mission: **drive the project to v1.0** by executing the
goals in `docs/developer-guide/project/goals.md`, one work item at a time.

### Step 1 — Determine current state (always do this first)

1. Read `docs/developer-guide/project/goals.md` and find the first goal with
   unchecked items.
2. Verify against reality before starting anything:
   - `gh run list --limit 10` — is CI green? A failing required workflow
     always outranks feature work.
   - `gh pr list` — is there an open PR for an item already in progress?
     Finish or unblock it before starting new work.
   - Check the item off in `goals.md` if it turns out to be already done
     (docs drift is a known problem in this repo — trust code over docs).
3. Select exactly **one** work item: the topmost unchecked item of the
   lowest-numbered goal that is actionable today. Skip items blocked on
   external decisions and note why.

### Step 2 — Execute the item end-to-end

- **Never commit to `main`.** Create `feature/<name>`, `fix/<name>`, or
  `chore/<name>` branch (worktree under `.claude/worktrees/` if isolation
  helps).
- Plan briefly, then implement following existing patterns
  (`docs/superpowers/specs/2026-04-10-codebase-consistency-standards-design.md`
  for module layout). Reuse existing utilities; search before writing new
  code.
- Write tests first where practical. Server: `cargo test` with `#[sqlx::test]`
  isolation. Client: `bun run test:run`. New SQL queries: run
  `cargo sqlx prepare --workspace` from the repo root and commit `.sqlx/`.
- Quality gates before push (all must pass):
  1. `cargo fmt --check && SQLX_OFFLINE=true cargo clippy -- -D warnings`
  2. `cargo test` (server) / `bun run test:run` (client) — whichever is touched
  3. `cargo deny check licenses` if dependencies changed
  4. Update `CHANGELOG.md` `[Unreleased]` for user-visible changes
- Respect the hard constraints in `CLAUDE.md`: no GPL/LGPL deps, UI contrast
  rules, server-side input validation, performance targets.

### Step 3 — Ship

1. Push the branch, open a PR (`gh pr create`) with a body that names the
   goal and item it completes. Conventional commit title
   (`type(scope): subject`, ≤72 chars).
2. Request a code review for significant changes (new modules, auth/crypto,
   API changes) and address findings.
3. Wait for CI. If CI fails, fix it — do not abandon the PR.
4. Merge with `gh pr merge --squash`, then clean up:
   `git worktree remove` (if used), delete local + remote branch,
   `git fetch --prune`.
5. Update `docs/developer-guide/project/goals.md` on the same PR (or a tiny
   follow-up PR): check the item off, add a one-line note if scope changed.

### Step 4 — Report and stop

End the session with a short report: what shipped (PR link), what's next in
the queue, and any blockers needing a human decision. **One merged item per
run is success.** Do not start a second item unless the first was trivial
(<30 min) and CI is green.

### Standing priorities (override item order)

1. **Red CI on `main`** — fix immediately, before anything else.
2. **Security advisories** (cargo deny / audit failures) — same day.
3. **Stalled open PRs** (#567 webrtc-rs, #568 keyring) — unblock before new work.
4. Then: goals in numeric order (G1 → G7).

### Escalate to the human (don't decide alone)

- Anything spending money or creating external accounts (Stripe, Sentry SaaS,
  app stores, code-signing certificates).
- Deploying to the VPS (`kaiku.pmind.de`) — deploys are manual and
  human-triggered. Never build Rust on the VPS.
- Dropping platform support (e.g., the Windows/libvpx decision, TD-25).
- The SaaS / billing go-no-go (Goal 6 item 5) and the i18n + iOS decisions
  (Goal 7).
- Any change to license policy or E2EE/crypto architecture.

### Definition of done (v1.0)

The mission is complete when every box in `goals.md` Goals 1–5 plus Goal 6
items 1–2 is checked, the Goal 7 gate questions have recorded decisions, and
a `v1.0.0` release exists with signed desktop artifacts, an upgrade playbook,
and a public changelog. Until then, every run moves one item closer.

---

## Usage

- **One-shot:** paste the prompt into a fresh session.
- **Recurring:** `/loop` with this prompt, or a scheduled agent — one merged
  PR per iteration keeps reviews digestible.
- **Parallel:** safe to run two sessions only if they pick items from
  different goals and use separate worktrees.
