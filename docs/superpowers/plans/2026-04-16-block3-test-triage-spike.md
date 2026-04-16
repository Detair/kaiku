# Block 3 — Android Test-Failure Triage Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the 13 pre-existing Android unit-test failures (carved out of Workstream A as "root cause unknown") into a classification document that clusters them by root cause, estimates per-cluster fix size, and seeds follow-up workstream specs. Optionally inline S-rated fixes if any cluster resolves in ≤30 minutes of code change.

**Architecture:** Bounded investigation spike. 90-minute wall-clock budget for investigation; additional ≤30-minute budget for inline fixes (120-min total worst case). Primary deliverable is a triage doc at `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md`. Optional secondary deliverable: test fix commits for S-rated clusters.

**Tech Stack:** Gradle 8.x, JUnit 4, MockK, Turbine, Hilt test, kotlinx-coroutines-test. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 3.

**Parallelization safe:** Yes — runs in parallel with Block 2 and Block 4 once Block 1 has merged. The only dependency is that `main`'s CI is green so baseline measurements are trustworthy.

---

## Pre-flight Check (BLOCKING)

- [ ] **Verify Block 1 has merged**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin
git log origin/main --oneline | grep -E "ci-drift|CI drift|RUSTSEC-2026-0099" | head -3
```

Expected: a commit referencing the Block 1 CI drift fix. Block 1 need not land before Block 3 *starts*, but the triage should run against `main` with Block 1 applied so the baseline is stable.

- [ ] **Verify the 13 failures are still present**

Set up Java/Android env once:

```bash
export JAVA_HOME="$HOME/.local/share/jdk/jdk-17.0.18+8"
export ANDROID_HOME="$HOME/.local/share/android-sdk"
export PATH="$JAVA_HOME/bin:$PATH"
```

From a fresh checkout of `origin/main`:

```bash
git worktree add /tmp/kaiku-main-check origin/main  # temporary, deleted after check
cd /tmp/kaiku-main-check/mobile/android
./gradlew :app:testDebugUnitTest --rerun-tasks 2>&1 | tail -5
python3 -c "
import xml.etree.ElementTree as ET, glob
total=skips=fails=errors=0
per = []
for f in sorted(glob.glob('app/build/test-results/testDebugUnitTest/*.xml')):
    r = ET.parse(f).getroot()
    t = int(r.get('tests',0)); s = int(r.get('skipped',0)); fl = int(r.get('failures',0)); e = int(r.get('errors',0))
    total += t; skips += s; fails += fl; errors += e
    if fl or e: per.append((r.get('name'), fl+e))
print(f'TOTAL tests={total} skipped={skips} failures={fails} errors={errors}')
for n, c in per: print(f'  {n}: {c}')
"
```

Expected: 13 failures across `AuthStateTest` (2), `AuthFlowTest` (5), `MessageFlowTest` (5), `QrLoginFlowTest` (1). Total test count may have increased from the Workstream A baseline of 174 (Block 4's un-ignore would add 1 to the total once it lands).

Clean up:

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove /tmp/kaiku-main-check
```

If the failure count changed materially (different test classes, totally different count), **STOP** — a recent commit may have landed on `main` that resolved or introduced failures. Re-baseline before starting the spike.

---

## Worktree Setup (run once after pre-flight passes)

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/test-failure-triage -b docs/android-test-triage origin/main
cd .claude/worktrees/test-failure-triage

export JAVA_HOME="$HOME/.local/share/jdk/jdk-17.0.18+8"
export ANDROID_HOME="$HOME/.local/share/android-sdk"
export PATH="$JAVA_HOME/bin:$PATH"
```

Working branch: `docs/android-test-triage`. Working directory for all tasks: `/home/detair/GIT/detair/kaiku/.claude/worktrees/test-failure-triage`.

---

## File Map

| Path | Action |
|------|--------|
| `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md` | Create — triage deliverable (Task 4) |
| `mobile/android/app/src/test/java/.../AuthStateTest.kt` | Possibly modify (Task 5, only if S-rated) |
| `mobile/android/app/src/test/java/.../AuthFlowTest.kt` | Possibly modify (Task 5, only if S-rated) |
| `mobile/android/app/src/test/java/.../MessageFlowTest.kt` | Possibly modify (Task 5, only if S-rated) |
| `mobile/android/app/src/test/java/.../QrLoginFlowTest.kt` | Possibly modify (Task 5, only if S-rated) |

---

## Task 1: Capture full failure output (investigation ~15 min)

**Goal:** Generate a complete, parseable record of every failing test — name, assertion, exception type, stack trace top frames, test file path.

- [ ] **Step 1: Run the suite with XML + stdout capture**

```bash
cd mobile/android
./gradlew :app:testDebugUnitTest --rerun-tasks 2>&1 | tee /tmp/android-test-run.log
```

Ignore the overall exit code (non-zero = at least one failure, which is expected). The run populates `app/build/test-results/testDebugUnitTest/*.xml` with structured results.

- [ ] **Step 2: Extract per-failure metadata from the XML reports**

```bash
python3 <<'PY' > /tmp/android-failures.tsv
import xml.etree.ElementTree as ET, glob
print("class\ttest_name\tfailure_type\tmessage\tfirst_stack_frame")
for f in sorted(glob.glob('app/build/test-results/testDebugUnitTest/*.xml')):
    root = ET.parse(f).getroot()
    cls_name = root.get('name')
    for tc in root.findall('testcase'):
        for tag in ('failure', 'error'):
            node = tc.find(tag)
            if node is not None:
                msg = (node.get('message') or '').replace('\t', ' ').replace('\n', ' | ')[:200]
                typ = node.get('type') or tag
                body = (node.text or '').strip().split('\n')
                frame = next((l.strip() for l in body if l.strip().startswith('at ')), '')[:200]
                print(f"{cls_name}\t{tc.get('name')}\t{typ}\t{msg}\t{frame}")
PY
cat /tmp/android-failures.tsv | head -20
wc -l /tmp/android-failures.tsv
```

Expected: 14 lines (1 header + 13 failure rows). If the row count is off, verify the XML parse caught every `<failure>`/`<error>` element.

- [ ] **Step 3: Also capture full stack traces for each failure (searchable later)**

```bash
for f in app/build/test-results/testDebugUnitTest/*.xml; do
    python3 <<PY
import xml.etree.ElementTree as ET, sys
r = ET.parse('$f').getroot()
for tc in r.findall('testcase'):
    for tag in ('failure', 'error'):
        n = tc.find(tag)
        if n is not None:
            print(f"=== {r.get('name')}.{tc.get('name')} [{tag}/{n.get('type')}]")
            print((n.text or '').strip())
            print()
PY
done > /tmp/android-failures-full.txt
wc -l /tmp/android-failures-full.txt
```

Expected: several hundred lines (13 stack traces × many frames each).

- [ ] **Step 4: Confirm the 4 test source files exist and note their paths**

```bash
find mobile/android/app/src/test/java -type f \( -name 'AuthStateTest.kt' -o -name 'AuthFlowTest.kt' -o -name 'MessageFlowTest.kt' -o -name 'QrLoginFlowTest.kt' \)
```

Expected: four matching files. Record the full paths — Task 4 cites them.

---

## Task 2: Cluster the failures (investigation ~30 min)

**Goal:** Identify shared root-cause signals across the 13 failures. Output: 1-3 candidate clusters with evidence.

- [ ] **Step 1: Bucket failures by the "first differential frame"**

For each failure in `/tmp/android-failures-full.txt`, find the first stack frame that points into Kaiku's own code (not Kotlin/JUnit/MockK framework). Group failures whose differential frame is the same file/line.

```bash
# Manually scan /tmp/android-failures-full.txt. For each === block, identify:
#   - exception type
#   - deepest non-framework frame (grep -E "at io.wolftown.kaiku")
# Build a mental (or /tmp/android-clusters.md) table:
#   Cluster A (shared fixture): AuthFlowTest×5, MessageFlowTest×5, QrLoginFlowTest×1 -- all fail at <same frame>
#   Cluster B (state assertion): AuthStateTest×2 -- distinct frame
```

- [ ] **Step 2: Check for shared `@Before` / `setUp()` infrastructure**

Common failure modes:
- A Hilt `@HiltAndroidTest` component that depends on a now-renamed production binding.
- A `MockWebServer` instance whose expected endpoints changed.
- A `DataStore` fixture with a schema drift.
- A `@Before` block that initialises `Dispatchers.setMain` but doesn't `resetMain` on failure paths.

Read the `@Before` and class-level setup of each failing test class:

```bash
for f in $(find mobile/android/app/src/test/java -type f \( -name 'AuthStateTest.kt' -o -name 'AuthFlowTest.kt' -o -name 'MessageFlowTest.kt' -o -name 'QrLoginFlowTest.kt' \)); do
    echo "=== $f"
    grep -nE "@Before|@After|setUp|tearDown" "$f" | head -10
done
```

Also grep for a shared test base:

```bash
find mobile/android/app/src/test -type f -name '*.kt' | xargs grep -lE "HiltAndroidTest|class .*TestBase|class .*TestHelper|TestApplication" | head -10
```

- [ ] **Step 3: Check recent git history for test-related changes**

```bash
git log --oneline --since='2026-03-01' -- mobile/android/app/src/test/ | head -20
git log --oneline --since='2026-03-01' -- mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/ mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/auth* | head -20
```

If a recent commit changed production data shape without updating test fixtures, that's a likely root cause.

- [ ] **Step 4: Assign each failure to a cluster**

For each of the 13 failures, tag one of:
- **Cluster name** (e.g., "Shared integration-test Hilt component")
- **Root cause hypothesis** (1 sentence, backed by the frame + shared-infra evidence)
- **Size estimate**: `S` (<30 min inline fix), `M` (<4h, its own workstream), `L` (>4h, commissioned spec with deeper spike)

If fewer than 13 assignments after this pass, create a "Cluster: unclassified" for the remainder and mark size `L` by default.

- [ ] **Step 5: Budget checkpoint**

After this step, **90-minute investigation budget is likely consumed**. If you have time remaining and any cluster is rated `S`, proceed to Task 5 (fix commits). Otherwise skip straight to Task 4 (write the doc).

**If investigation exceeded 90 minutes**: stop classifying, write what you have, explicitly mark unclassified failures as `status: not yet triaged` in the doc, and commission a follow-up spike.

---

## Task 3: (Skipped — merged into Tasks 2 and 4)

Task numbering matches the spec's commit plan. This slot reserved for "diagram the clusters" if the investigation produces a visual artifact — usually not needed for a triage doc.

---

## Task 4: Write the triage doc

**Files:**
- Create: `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md`

- [ ] **Step 1: Scaffold the document**

```bash
mkdir -p docs/developer-guide/testing
```

- [ ] **Step 2: Fill in the template**

The doc must contain these sections:

```markdown
# Android Unit Test Failure Triage — 2026-04-16

**Context:** Workstream A (#534) carved 13 pre-existing Android unit test failures out of scope. This document classifies those 13 into root-cause clusters with per-cluster recommended next steps.

**Spec:** `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 3.

**Budget:** 90-minute investigation spike (per spec). [Note if exceeded.]

## Baseline: all 13 failures

| # | Class | Test method | Exception type | First non-framework frame | Cluster |
|---|-------|-------------|----------------|---------------------------|---------|
| 1 | `AuthStateTest` | `<method>` | `<type>` | `<file:line>` | `<A/B/…>` |
| … | | | | | |

(One row per failure. Cluster column uses letters A, B, C as assigned.)

## Clusters

### Cluster A — <Hypothesis in 5-10 words>

**Failures covered:** <N of 13> (rows <1, 3, 5, …>)

**Evidence:**
- <shared stack frame / shared fixture / shared setup line>
- <git-blame observation linking to recent change, if any>

**Root cause hypothesis (1-2 sentences):**
<statement>

**Size rating:** `S` | `M` | `L`

**Recommended action:**
- If `S`: inline fix in this PR. Describe the change in 1-2 lines.
- If `M` or `L`: seed a follow-up workstream. Propose a one-paragraph spec outline.

### Cluster B — <Hypothesis>
…

## Recommended next steps

- For each `S`-rated cluster: fix committed in this PR (link to commit SHAs once known).
- For each `M`/`L`-rated cluster: follow-up workstream proposed.
  - Proposed spec stub: <cluster name> — <one-sentence goal>.
  - Suggested owner/area: <auth | messaging | qr-login | data-local>.

## Out of scope for this spike

- Actually fixing `M`/`L`-rated clusters (by design).
- Adding test coverage for any production code uncovered as buggy during triage.
- Robolectric / `androidTest/` migration.
```

- [ ] **Step 3: Populate the table from `/tmp/android-failures.tsv`**

Convert each row of the TSV into a table row in the "Baseline" section. Use markdown pipe tables. Cluster column references the assignments from Task 2 Step 4.

- [ ] **Step 4: Commit**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/test-failure-triage
git add docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md
git commit -m "docs(client): triage 13 pre-existing Android unit test failures"
```

---

## Task 5 (CONDITIONAL): Apply S-rated cluster fixes

**Skip this task entirely** if no cluster is rated `S`. Triage-doc-only PR is a complete deliverable.

**If one or more clusters are rated `S`:**

- [ ] **Step 1: Re-verify the 30-minute fix budget**

Count of S-rated clusters × estimated fix time per cluster ≤ 30 minutes. If the total exceeds 30 minutes, downgrade the cheapest-to-defer cluster to `M` and re-run this budget check. Do not violate the 30-minute ceiling.

- [ ] **Step 2: Apply one cluster's fix at a time**

For each `S`-rated cluster, in any order:

1. Read the failing test file(s) and the production code they touch.
2. Write the minimal fix — usually: update a `@Before` stub, correct a mock expectation, fix a data-shape mismatch.
3. Run the affected test class(es):
   ```bash
   cd mobile/android
   ./gradlew :app:testDebugUnitTest --tests 'io.wolftown.kaiku.<pkg>.<ClassName>' --rerun-tasks 2>&1 | tail -10
   ```
   Expected: the previously-failing tests in that class now pass.
4. Run the full suite to confirm no new regressions:
   ```bash
   ./gradlew :app:testDebugUnitTest --rerun-tasks 2>&1 | tail -3
   python3 <<'PY'
   import xml.etree.ElementTree as ET, glob
   total=skips=fails=errors=0
   for f in sorted(glob.glob('app/build/test-results/testDebugUnitTest/*.xml')):
       r = ET.parse(f).getroot()
       total += int(r.get('tests',0)); skips += int(r.get('skipped',0))
       fails += int(r.get('failures',0)); errors += int(r.get('errors',0))
   print(f'TOTAL tests={total} skipped={skips} failures={fails} errors={errors}')
PY
   ```
   Expected: failure count decreased by the number of tests in the fixed cluster; no new failures outside the remaining clusters.

5. Commit:
   ```bash
   git add mobile/android/app/src/test/java/io/wolftown/kaiku/<path>/<ClassName>.kt
   git commit -m "test(client): <cluster name> — <1-sentence fix description>"
   ```

- [ ] **Step 3: Update the triage doc**

After each S-rated fix commits, edit `2026-04-16-android-test-failure-triage.md`:
- In the cluster's "Recommended action" section, replace the prospective description with the actual commit SHA + brief note.
- In the "Recommended next steps" section, cross off the cluster.

Commit the doc update as part of the fix commit (stage both files together) OR as a trailing commit — `docs(client): update triage doc with applied fixes`.

---

## Final Verification (before opening PR)

- [ ] **Triage doc exists and is complete**

```bash
test -f docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md && \
  grep -cE '^| [0-9]+ ' docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md
```

Expected: file exists; baseline table row count equals 13 (one line per failure).

- [ ] **Test-suite status matches the doc's claims**

```bash
cd mobile/android
./gradlew :app:testDebugUnitTest --rerun-tasks 2>&1 | tail -3
```

Expected:
- If zero S-rated fixes applied: full-suite failure count still 13, in the same allowlist of classes.
- If S-rated fixes applied: failure count reduced by exactly the number of tests the doc claims are now fixed.

- [ ] **Commit log review**

```bash
git log --oneline origin/main..HEAD
```

Expected (minimum): one `docs(client): triage …` commit. Up to 3 additional `test(client): <cluster>` commits if S-rated fixes applied, plus an optional trailing `docs(client): update triage doc with applied fixes`.

- [ ] **Push and open PR**

```bash
git push -u origin docs/android-test-triage
gh pr create --base main --head docs/android-test-triage \
  --title "docs(client): Android unit test failure triage + targeted fixes" \
  --body "$(cat <<'EOF'
## Summary

Block 3 of Phase 2.5 open-topics cleanup. Classifies the 13 pre-existing Android unit test failures (carved out of Workstream A) into root-cause clusters with per-cluster size ratings and next steps.

Primary deliverable: `docs/developer-guide/testing/2026-04-16-android-test-failure-triage.md`.

Optional inline fixes: [list S-rated clusters that were fixed, or "none — all clusters deferred to follow-up workstreams"].

Spec: `docs/superpowers/specs/2026-04-16-open-topics-cleanup-design.md` — Block 3.

## Budget

- Investigation: [actual minutes] min (budget 90).
- Inline fixes: [actual minutes] min (budget 30).

## Remaining deferred clusters

[Enumerate M/L-rated clusters with one-line proposed follow-up workstream descriptions.]

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Wait for CI green, merge**

```bash
gh pr checks <PR_NUMBER> --watch
gh pr merge <PR_NUMBER> --squash
```

---

## Post-merge cleanup

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/test-failure-triage
git branch -D docs/android-test-triage
git push origin --delete docs/android-test-triage
git fetch --prune
```

Follow-up: for each M/L-rated cluster enumerated in the triage doc's "Recommended next steps", file a proposed workstream spec under `docs/superpowers/specs/YYYY-MM-DD-<cluster>-design.md` when the user commissions it.

---

## Notes for the implementer

- **The 90-minute investigation budget is load-bearing.** If you finish the classification faster than 90 minutes, don't extend into deeper debugging — finalize the doc and consider Task 5. If you hit 90 minutes with unclassified failures, ship the doc in its partial state and explicitly mark `status: not yet triaged`.
- **Do not expand scope mid-task.** If Cluster A is 10 failures rated M, don't decide to fix them "because it's right there" — that converts an M into an L due to context-switching, blows the budget, and breaks the spec's deliberate out-of-scope carve-out.
- **Clusters of one failure are valid.** Sometimes there's no shared root cause. A cluster of exactly one failing test with its own hypothesis is a legitimate classification outcome.
- **Prefer M-rated proposals over S-rated fixes when uncertain.** Ship the classification and let follow-up workstreams own real fixes, rather than half-fixing a cluster in a 30-minute budget and leaving a landmine.
- **Test fixtures vs. production bugs.** If triage reveals an actual production bug (e.g., `AuthFlowTest` fails because `AuthRepository.refreshToken()` really does crash on empty input), document it as a production bug in the cluster's root-cause field and rate it at least M. Do not silently fix production code inside a triage spike.
