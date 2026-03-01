# CachyOS Dev Setup Manual Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a reusable Markdown manual and a consistent Makefile target contract for per-project isolated development environments on CachyOS.

**Architecture:** Keep host tooling minimal and run project work inside per-project Distrobox containers. Document the pattern in a portable manual and align this repository's Makefile with the standard target names so the workflow is consistent across repos.

**Tech Stack:** GNU Make, Bash, Distrobox, Podman/Docker Compose, Rust, Bun, Node.js, sqlx

---

### Task 1: Add reusable manual document

**Files:**
- Create: `docs/development/cachyos-dev-environment-manual.md`
- Reference: `docs/plans/2026-02-15-cachyos-multi-project-dev-environment-design.md`

**Step 1: Write the failing test**

Use a content check command that fails if required manual sections are missing.

```bash
rg -n "^# |^## " docs/development/cachyos-dev-environment-manual.md
```

Expected missing sections before file exists:
- Host baseline setup
- Container naming convention
- Standard Makefile target contract
- New-project onboarding checklist
- Troubleshooting and reset playbook

**Step 2: Run test to verify it fails**

Run:

```bash
test -f docs/development/cachyos-dev-environment-manual.md
```

Expected: FAIL (exit code non-zero, file does not exist).

**Step 3: Write minimal implementation**

Create `docs/development/cachyos-dev-environment-manual.md` with:
- purpose and non-goals
- host package install instructions for CachyOS/Arch (`pacman`)
- Distrobox workflow (`create`, `enter`, `export`)
- repo-scoped env and service isolation rules
- canonical Make targets and expected behavior
- copy/paste starter Makefile snippet for cross-project reuse
- verification checklist and troubleshooting

**Step 4: Run test to verify it passes**

Run:

```bash
test -f docs/development/cachyos-dev-environment-manual.md && rg -n "Host baseline|Distrobox|Makefile|Troubleshooting" docs/development/cachyos-dev-environment-manual.md
```

Expected: PASS with matching section lines.

**Step 5: Commit**

```bash
git add docs/development/cachyos-dev-environment-manual.md
git commit -m "docs(infra): add reusable CachyOS dev environment manual"
```

### Task 2: Align Makefile with standard target contract

**Files:**
- Modify: `Makefile`
- Test: `Makefile`

**Step 1: Write the failing test**

Check for required targets that should exist for cross-repo consistency.

```bash
rg -n "^(init|bootstrap|doctor|services-up|services-down|migrate|lint|fmt):" Makefile
```

Expected: FAIL for missing aliases (`init`, `bootstrap`, `doctor`, `services-up`, `services-down`, `migrate`).

**Step 2: Run test to verify it fails**

Run:

```bash
make init
```

Expected: FAIL with "No rule to make target 'init'".

**Step 3: Write minimal implementation**

Update `Makefile` by adding non-breaking aliases/wrappers:
- `init` -> current setup entrypoint
- `bootstrap` -> existing install/setup flow
- `services-up`/`services-down` -> existing docker targets
- `migrate` -> existing `db-migrate`
- `doctor` -> lightweight dependency and service checks

Do not remove existing targets; keep backward compatibility.

**Step 4: Run test to verify it passes**

Run:

```bash
make -n init bootstrap services-up services-down migrate doctor lint fmt
```

Expected: PASS with command previews for all targets.

**Step 5: Commit**

```bash
git add Makefile
git commit -m "chore(infra): add standard dev workflow make targets"
```

### Task 3: Link manual from existing setup docs

**Files:**
- Modify: `docs/development/setup.md`
- Test: `docs/development/setup.md`

**Step 1: Write the failing test**

Search for a link/reference to the new manual.

```bash
rg -n "cachyos-dev-environment-manual" docs/development/setup.md
```

Expected: FAIL (no reference yet).

**Step 2: Run test to verify it fails**

Run the command above and confirm no matches.

**Step 3: Write minimal implementation**

Add a short "Standardized Multi-Project Setup" section in `docs/development/setup.md` linking to:
- `docs/development/cachyos-dev-environment-manual.md`

Keep quick-start instructions intact.

**Step 4: Run test to verify it passes**

Run:

```bash
rg -n "cachyos-dev-environment-manual" docs/development/setup.md
```

Expected: PASS with one matching line.

**Step 5: Commit**

```bash
git add docs/development/setup.md
git commit -m "docs(infra): link standardized CachyOS setup manual"
```

### Task 4: Validate final workflow end-to-end

**Files:**
- Modify if needed: `Makefile`, `docs/development/cachyos-dev-environment-manual.md`

**Step 1: Write the failing test**

Dry-run required make targets and ensure no missing rules.

```bash
make -n init bootstrap doctor services-up services-down migrate dev test lint fmt
```

Expected before fixes: any missing-target failure.

**Step 2: Run test to verify it fails**

Run the command and capture first failure.

**Step 3: Write minimal implementation**

Fix target wiring or docs mismatches discovered during dry run.

**Step 4: Run test to verify it passes**

Run:

```bash
make -n init bootstrap doctor services-up services-down migrate dev test lint fmt
```

Expected: PASS with full command preview.

**Step 5: Commit**

```bash
git add Makefile docs/development/cachyos-dev-environment-manual.md docs/development/setup.md
git commit -m "docs(infra): standardize multi-project dev environment workflow"
```
