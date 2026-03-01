# Git Workflow Design

> **For Claude:** Follow these rules for all git operations.

**Goal:** Establish consistent git practices for commit quality, branch management, code review, and transparency.

**Approach:** Guidelines-based with agent enforcement (no git hooks).

---

## Commit Convention

### Format

```
type(scope): subject

[optional body]

[optional footer]
```

### Types

| Type | Use for |
|------|---------|
| `feat` | New features or capabilities |
| `fix` | Bug fixes |
| `docs` | Documentation only |
| `refactor` | Code changes that don't add features or fix bugs |
| `test` | Adding or updating tests |
| `chore` | Build, CI, dependencies, tooling |
| `perf` | Performance improvements |
| `style` | Formatting, whitespace (no logic change) |

### Scopes

| Scope | Covers |
|-------|--------|
| `auth` | Authentication, JWT, OIDC, MFA |
| `voice` | WebRTC, SFU, audio processing |
| `chat` | Messages, channels, DMs |
| `db` | Database, migrations, queries |
| `api` | REST endpoints, routing |
| `ws` | WebSocket handlers |
| `ratelimit` | Rate limiting |
| `infra` | Docker, CI/CD, deployment |
| `client` | Tauri/Solid.js frontend |
| `crypto` | E2EE, encryption |

### Rules

- Subject line max 72 characters
- Imperative mood ("add feature" not "added feature")
- Body optional but encouraged for non-trivial changes
- Breaking changes: add `!` after scope, e.g., `feat(api)!: change response format`

### Examples

Simple fix:
```
fix(auth): correct JWT expiry calculation
```

Feature with context:
```
feat(auth): add MFA TOTP support

Users requested additional security for sensitive servers.
Implements RFC 6238 TOTP with 30-second windows.

Closes #42
```

Breaking change:
```
feat(api)!: change message response format

BREAKING CHANGE: Message.content is now Message.body
Old clients will fail to parse messages.
```

---

## Branch Strategy

### Naming Convention

```
feature/<short-name>    # New features
fix/<issue-or-name>     # Bug fixes
refactor/<area>         # Refactoring work
docs/<topic>            # Documentation
```

### Worktree Workflow

Use git worktrees to work on multiple features simultaneously:

```bash
# Create worktree for new feature
git worktree add ../canis-feature-xyz feature/xyz

# List active worktrees
git worktree list

# Remove when done (after merge)
git worktree remove ../canis-feature-xyz
```

### Directory Convention

```
~/GIT/
├── canis/                    # Main worktree (main branch)
├── canis-feature-xyz/        # Feature worktree
├── canis-fix-auth-bug/       # Fix worktree
└── canis-refactor-db/        # Refactor worktree
```

### Rules

- Main worktree stays on `main` branch
- One worktree per feature/fix
- Clean up worktrees after merging
- Never commit directly to `main` in feature worktrees

### Branch Cleanup

- Branches merged to main: delete immediately
- Branches inactive >30 days: review and archive or delete

---

## Pre-Push Quality Gates

Before pushing any code:

### 1. Run Tests

```bash
# Server tests
cd server && cargo test

# Client tests (if changes in client/)
cd client && bun test
```

All tests must pass. Fix failures before pushing.

### 2. Run Linting

```bash
# Rust
cargo fmt --check
cargo clippy -- -D warnings

# TypeScript (if client changes)
cd client && bun run lint
```

### 3. Self-Review Checklist

Before committing, verify:

- [ ] Code compiles without warnings
- [ ] Tests pass
- [ ] No secrets or credentials in code
- [ ] Commit message follows convention
- [ ] Changes match the intended scope (no unrelated changes)
- [ ] Error handling is appropriate
- [ ] No TODO/FIXME left unaddressed (or tracked in issue)

### 4. Code Review for Significant Changes

For non-trivial changes, perform code review before pushing.

**What counts as "significant":**
- New files or modules
- Changes to auth, crypto, or security code
- Database schema changes
- API contract changes
- Performance-critical paths

---

## Transparency & Auditability

### Commit Body for Context

For non-trivial changes, explain *why*:

```
feat(chat): add message reactions

Enables users to react to messages with emoji without sending
a full reply. Limited to 20 unique reactions per message.

Relates to #67
```

### Linking to Context

- Reference issues: `Closes #42`, `Fixes #42`, `Relates to #42`
- Reference PRs: `See PR #38 for discussion`
- Reference docs: `See docs/plans/2026-01-15-mfa-design.md`

### Design Decisions

- Major features should have a design doc in `docs/plans/`
- Design docs are committed before implementation starts
- Commit references the design doc

### Change History Practices

- Avoid force-push to `main` (preserves history)
- Use merge commits for features (shows branch context)
- Squash only when branch history is truly noisy
- Never rewrite history after push to shared branches

### PR Descriptions

```markdown
## Summary
- What changed and why

## Test Plan
- How to verify the change works

## Breaking Changes
- None / List them
```

---

## Quick Reference

### Commit Template
```
type(scope): subject (max 72 chars)

Why this change is needed.

Closes #XX
```

### Common Commands
```bash
# Create feature worktree
git worktree add ../canis-feature-name -b feature/name

# Pre-push checks
cargo fmt --check && cargo clippy -- -D warnings && cargo test

# Clean up after merge
git worktree remove ../canis-feature-name
git branch -d feature/name
```
