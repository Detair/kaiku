# PROJECT KNOWLEDGE BASE

**Lifecycle:** Active
**Last Updated:** 2026-02-27

## Mandatory Start Policy

Before starting any non-trivial work, create and use a dedicated git worktree.

1. Ensure you are on an up-to-date base branch (`main` unless instructed otherwise).
2. Create a feature/docs/fix branch in a new worktree.
3. Perform all edits, tests, and commits from that worktree path.
4. Keep the primary checkout clean for parallel work and emergency fixes.

Recommended command pattern:

```bash
git worktree add -b "<branch-name>" "../canis-worktrees/<worktree-name>" main
```

## Critical Documentation Map

Read these first to avoid redundant searching and to stay aligned with project direction:

- Roadmap and phase status: `docs/project/roadmap.md`
- Active plan lifecycle rules: `docs/plans/PLAN_LIFECYCLE.md`
- Phase 7 observability design: `docs/plans/2026-02-15-phase-7-a11y-observability-design.md`
- Phase 7 observability implementation baseline: `docs/plans/2026-02-15-phase-7-a11y-observability-implementation.md`
- OTel and Grafana reference architecture: `docs/plans/2026-02-15-opentelemetry-grafana-reference-design.md`
- Operational safety implementation plan: `docs/plans/2026-02-15-operational-safety-implementation-plan.md`

## Quick Execution Checklist

Use this checklist at task start:

- [ ] Worktree created and branch named per convention
- [ ] Roadmap phase and target item confirmed
- [ ] Relevant design + implementation docs opened
- [ ] Standards profile considered (OTel, W3C Trace Context, WCAG where applicable)
- [ ] Verification commands identified before first code change
