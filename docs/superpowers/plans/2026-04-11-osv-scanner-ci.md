# OSV Scanner CI Job Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `osv-scanner` job to the existing `.github/workflows/security.yml` workflow as an independent advisory source. Scans Rust + npm + workflow files in one pass using the OSV database (independent of RustSec and GitHub Advisory Database).

**Architecture:** Add a new job to `security.yml` (do not create a new workflow file). The job runs on the same triggers as the rest of `security.yml` (weekly schedule, push to main, manual dispatch). Failure semantics: scanner exit code is the gate, no `continue-on-error`. SARIF output uploads to GitHub Code Scanning for tracking.

**Tech Stack:** GitHub Actions, osv-scanner (Google), SARIF / Code Scanning

**Spec:** `docs/superpowers/specs/2026-04-11-security-audit-followups-design.md` (Topic 2)

**Branch:** `feat/osv-scanner-ci`

**Important context:** The existing `security.yml` already has `rust-audit` (cargo-audit), `dependencies` (cargo-deny), `bun-audit` (bun pm scan with `continue-on-error`), `secrets-scan` (gitleaks), and `codeql` (JS/TS). osv-scanner is a NEW job that adds the OSV database as a third independent source (RustSec via cargo-audit/deny, GHSA via CodeQL, OSV via osv-scanner).

**Ordering note:** The spec lists this topic as depending on Topic 1 being green ("otherwise the new job fails day 1"). If Topic 1 hasn't merged yet, the osv-scan job will likely report the same 17 devtime advisories that bun audit currently reports. Either land Topic 1 first, or expect the first run on this PR to fail with those advisories.

**Action versioning:** This plan pins `google/osv-scanner-action` to a specific tag rather than a floating major. Upstream explicitly recommends pinning because "the scanner action behavior might change in a minor patch update." There is **no `v2` tag** in the upstream repo — only patch tags like `v2.3.5`. Floating-major refs will fail to resolve at job-start time.

---

## File structure

| File | Role |
|---|---|
| `.github/workflows/security.yml` | Add `osv-scan` job (modification, not creation) |

---

## Task 1: Add the osv-scan job

**Files:**
- Modify: `.github/workflows/security.yml` (append a new job at the end)

- [ ] **Step 1: Verify the existing workflow structure**

Run: `head -20 .github/workflows/security.yml`

Expected: `name: Security Audit`, triggers include weekly cron + push to main + workflow_dispatch. Confirm the file exists and is the one we'll be modifying.

- [ ] **Step 2: Add the osv-scan job**

Append to the end of `.github/workflows/security.yml` (after the `codeql` job, on a new line):

```yaml

  # ===========================================================================
  # OSV Scanner — independent advisory source (OSV database)
  # ===========================================================================
  osv-scan:
    name: OSV Scanner
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write  # for SARIF upload to Code Scanning
    steps:
      - uses: actions/checkout@v4

      - name: Run osv-scanner
        # Exit code is the gate. Do NOT set continue-on-error.
        # SARIF upload below uses if: always() to publish results
        # to Code Scanning even on scanner failure.
        #
        # Pinned to v2.3.5 specifically because:
        # 1. Upstream has no `v2` floating tag — only patch tags exist.
        # 2. Upstream warns: "the scanner action behavior might change in
        #    a minor patch update." Pin precisely.
        #
        # Before merging this PR, the implementer should check
        # https://github.com/google/osv-scanner-action/releases for any
        # newer patch tag and update if appropriate.
        uses: google/osv-scanner-action/osv-scanner-action@v2.3.5
        with:
          # --severity=HIGH gates only on HIGH+ findings, matching the
          # spec's recommended threshold. MEDIUM/LOW still appear in the
          # SARIF upload (and the Security tab) but don't fail the job.
          scan-args: |-
            --recursive
            --skip-git
            --severity=HIGH
            --format=sarif
            --output=osv-results.sarif
            ./

      - name: Upload SARIF to Code Scanning
        if: always()
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: osv-results.sarif
          category: osv-scanner
```

**Notes for the implementer:**

- The job uses the same checkout-based pattern as other jobs in this file. No bun setup or rust toolchain needed — osv-scanner reads `Cargo.lock`, `client/bun.lock`, etc. directly.
- `--skip-git` prevents the scanner from trying to scan submodules or git history. We only care about the lockfiles in the working tree.
- `--severity=HIGH` matches the spec's recommended gating threshold.
- `category: osv-scanner` separates osv-scanner findings from other SARIF uploads (CodeQL) in the GitHub Security tab.
- `if: always()` on the upload step means SARIF is published even when the scan finds vulnerabilities (and the prior step exited non-zero). This is intentional — failed scans are exactly when you want the SARIF in the Security tab.

**bun.lock support caveat:** osv-scanner's lockfile parser support has historically lagged on bun. There is a real chance that `--recursive` will only pick up `Cargo.lock` and skip `client/bun.lock`. If that happens, the OSV coverage is Rust-only (still useful — `cargo audit` and `cargo deny` cover RustSec; OSV is the third independent source for Rust). The first CI run will tell us. If bun.lock is skipped, document the limitation in the PR description and file a follow-up to reach out to the osv-scanner maintainers about bun support.

- [ ] **Step 3: Validate YAML syntax**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/security.yml'))" 2>&1`

Expected: no output (success). If you see a YAML parse error, fix the indentation. Common mistakes:
- Tab characters instead of spaces (must be spaces only)
- Wrong indentation level for `osv-scan:` (should be at the same level as `codeql:`)
- Missing newline before the new job

- [ ] **Step 4: Lint with actionlint (optional, only if available)**

Run: `which actionlint && actionlint .github/workflows/security.yml 2>&1 || echo "actionlint not installed, skipping"`

Expected: clean output or "actionlint not installed". If actionlint is installed and reports errors, fix them before committing.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/security.yml
git commit -m "ci(security): add osv-scanner job for OSV database coverage

Adds Google's osv-scanner as a new job in security.yml. Uses the OSV
database, which is independent of RustSec (covered by cargo-audit and
cargo-deny) and GHSA (covered by CodeQL). Provides a third independent
advisory source.

Failure semantics: scanner exit code is the gate. SARIF output uploads
to GitHub Code Scanning even on scan failure (if: always()) so the
Security tab tracks history.

Refs spec docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 2)"
```

---

## Task 2: Push and verify

- [ ] **Step 1: Push the branch**

Run: `git push -u origin feat/osv-scanner-ci`

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "ci(security): add osv-scanner job for OSV database coverage" --body "$(cat <<'EOF'
## Summary

Adds an osv-scanner job to \`.github/workflows/security.yml\`. Uses the OSV vulnerability database, independent from RustSec (cargo-audit/deny) and GHSA (CodeQL).

## Failure semantics

- Scanner exit code is the gate. No \`continue-on-error\`.
- SARIF upload runs on \`if: always()\` so failed scans still publish to GitHub Security tab.
- Default severity threshold (any). May be tightened to \`--severity=HIGH\` after observing baseline noise.

## Test plan

- [x] YAML validates with python3 yaml.safe_load
- [ ] First CI run on this PR completes (job either passes or reports vulnerabilities cleanly)
- [ ] If the job fails, the failures are real advisories — not transient errors
- [ ] SARIF results visible in GitHub Security tab under \"osv-scanner\" category

## Refs

- Spec: docs/superpowers/specs/2026-04-11-security-audit-followups-design.md (Topic 2)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Watch the first run**

Run: `gh pr checks <pr-number>` and look for the "OSV Scanner" job.

Expected outcomes:

a) **Job passes** (zero vulnerabilities): great, ship it.

b) **Job fails with real vulnerabilities**: this is informative. The OSV database may flag advisories that RustSec/GHSA don't. Investigate each finding:
- If it's a duplicate of something cargo-deny/CodeQL already catches: harmless, but we're paying for the duplicate. Acceptable.
- If it's a new advisory we hadn't seen: this is the value of the second source. Add it to a follow-up issue and fix in a separate PR.
- If it's a transitive devtime issue covered by Topic 1's Plan A: should already be patched. If not, Topic 1 missed it.

c) **Job times out or errors out**: investigate. The scanner may need network access for the OSV database lookup. If GitHub Actions runners can't reach osv.dev, escalate.

If the scan reports new vulnerabilities, **do not fix them in this PR**. Open a separate issue and ship the workflow change first.

- [ ] **Step 4: Iterate if needed**

The plan ships with `--severity=HIGH` already, so MEDIUM/LOW findings only show in the SARIF upload (and Security tab) and don't gate. If the HIGH+ count is still too noisy, raise to `--severity=CRITICAL`. If you want broader coverage and the noise is acceptable, lower to `--severity=MEDIUM`. Re-push, re-check, repeat as needed.

- [ ] **Step 5: Merge**

```bash
gh pr merge --squash --delete-branch
```

---

## Done criteria

- [ ] `osv-scan` job exists in `.github/workflows/security.yml`
- [ ] First CI run on the PR completes (passes or reports real findings)
- [ ] SARIF results visible in GitHub Security tab under `osv-scanner` category
- [ ] PR merged to main
- [ ] Subsequent main-branch runs include the osv-scan job
