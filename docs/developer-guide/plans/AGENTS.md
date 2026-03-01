# Implementation Plans

**Parent:** [../../../AGENTS.md](../../../AGENTS.md)

## Purpose

Implementation and design plans for features, following a two-document pattern: design doc followed by implementation plan. These documents guide development and serve as historical record of architectural decisions.

## Document Patterns

### Design Documents

**Naming:** `YYYY-MM-DD-feature-name-design.md`

**Purpose:** High-level feature design and decision-making

**Typical sections:**
- Problem statement
- Goals and non-goals
- Design options considered
- Chosen approach with rationale
- API/Protocol design
- Security considerations
- Performance implications
- Open questions

**Examples:**
- `2026-01-13-home-view-design.md`
- `2026-01-14-dm-voice-calls-design.md`
- `2026-01-18-git-workflow-design.md`

### Implementation Plans

**Naming:** `YYYY-MM-DD-feature-name-implementation.md` or `YYYY-MM-DD-feature-name-plan.md`

**Purpose:** Detailed step-by-step implementation guide

**Typical sections:**
- Overview (links to design doc)
- Prerequisites
- Implementation tasks (numbered, atomic)
- File changes required
- Database migrations
- Testing strategy
- Rollout plan

**Examples:**
- `2026-01-13-home-view-implementation.md`
- `2026-01-14-dm-voice-calls-implementation.md`
- `permission-system-implementation-2026-01-13.md`

### Lifecycle Metadata

When replacing older plans, update lifecycle metadata immediately:

- Superseded docs must include:
  - `**Lifecycle:** Superseded`
  - `**Superseded By:** <new-doc.md>`
- Replacement docs must include:
  - `**Lifecycle:** Active`
  - `**Supersedes:** <old-doc.md>`

Track all active/superseded pairs in `PLAN_LIFECYCLE.md`.

### Special Documents

**Agent Prompts:**
- `PHASE_3_AGENT_PROMPT.md` — Agent instructions for specific project phase
- `PHASE_3_IMPLEMENTATION.md` — Phase-specific implementation tasks

## Key Files

| File | Purpose | Status |
|------|---------|--------|
| `2026-01-13-home-view-design.md` | Home view feature design | Design |
| `2026-01-13-home-view-implementation.md` | Home view implementation plan | Implementation |
| `2026-01-14-dm-voice-calls-design.md` | DM voice calls design | Design |
| `2026-01-14-dm-voice-calls-implementation.md` | DM voice calls implementation | Implementation |
| `2026-01-14-guild-management-modal-design.md` | Guild management UI design | Design |
| `2026-01-14-guild-management-modal-plan.md` | Guild management implementation | Implementation |
| `2026-01-16-bun-migration-design.md` | Bun package manager migration design | Design |
| `2026-01-16-bun-migration.md` | Bun migration implementation | Implementation |
| `2026-01-16-information-pages-design.md` | Info pages design | Design |
| `2026-01-17-information-pages-implementation.md` | Info pages implementation | Implementation |
| `2026-01-17-persona-rework-design.md` | Code review personas redesign | Design |
| `2026-01-17-rate-limiting-implementation.md` | Rate limiting implementation | Implementation |
| `2026-01-18-git-workflow-design.md` | Git workflow conventions design | Design |
| `permission-system-design-2026-01-13.md` | Permission system design | Design |
| `permission-system-implementation-2026-01-13.md` | Permission system implementation | Implementation |
| `PHASE_3_AGENT_PROMPT.md` | Phase 3 agent instructions | Meta |
| `PHASE_3_IMPLEMENTATION.md` | Phase 3 task breakdown | Meta |

## For AI Agents

### Creating new plans

**1. Start with design doc:**
```bash
# Create design document
docs/plans/YYYY-MM-DD-feature-name-design.md
```

**Design doc template:**
```markdown
# Feature Name — Design

**Date:** YYYY-MM-DD
**Status:** Draft | In Review | Approved | Implemented

## Problem

What problem are we solving? Who has this problem?

## Goals

- Primary goal 1
- Primary goal 2

## Non-Goals

- Out of scope item 1
- Future consideration 2

## Design Options

### Option A: [Name]
**Pros:**
- ...

**Cons:**
- ...

### Option B: [Name]
...

## Chosen Approach

[Detailed design of chosen option]

### API Design
[If applicable]

### Database Schema
[If applicable]

### Security Considerations
[Required for all features]

### Performance Implications
[If relevant]

## Open Questions

- Question 1?
- Question 2?

## References

- Related specs: [ARCHITECTURE.md](../../specs/ARCHITECTURE.md)
- Related issues: #123, #456
```

**2. Create implementation plan:**
```bash
# Create implementation plan
docs/plans/YYYY-MM-DD-feature-name-implementation.md
```

**3. If replacing existing plans, update lifecycle metadata:**
```bash
# Update docs/plans/PLAN_LIFECYCLE.md
# Add Lifecycle/Supersedes headers in both old and new documents
python3 scripts/check_docs_governance.py
```

**Implementation plan template:**
```markdown
# Feature Name — Implementation Plan

**Date:** YYYY-MM-DD
**Design Doc:** [YYYY-MM-DD-feature-name-design.md](./YYYY-MM-DD-feature-name-design.md)
**Status:** Not Started | In Progress | Completed

## Overview

Brief summary linking to design doc.

## Prerequisites

- [ ] Design approved
- [ ] Dependencies resolved
- [ ] Test environment ready

## Implementation Tasks

### Phase 1: Foundation

1. **Task description**
   - File: `path/to/file.rs`
   - Action: What to do
   - Rationale: Why

2. **Next task**
   ...

### Phase 2: Core Implementation

...

### Phase 3: Testing & Polish

...

## Database Migrations

```sql
-- Migration SQL if applicable
```

## Testing Strategy

- Unit tests: [areas to cover]
- Integration tests: [scenarios]
- E2E tests: [user flows]

## Rollout Plan

1. Merge to main
2. Deploy to staging
3. Verify functionality
4. Deploy to production

## Verification Checklist

- [ ] All tests pass
- [ ] Linting clean
- [ ] Documentation updated
- [ ] Performance benchmarks met
```

### Using existing plans

**Reading plans:**
1. Start with design doc to understand "why"
2. Read implementation plan for "how" and "what"
3. Check status to see if complete or in-progress
4. Look for open questions or blocked items

**Updating plans:**
- Mark tasks as completed
- Add notes on deviations from plan
- Update status field
- Link to actual PRs/commits

**When implementation diverges:**
Add "Implementation Notes" section documenting actual approach vs. planned approach with rationale.

### Plan lifecycle

```
1. Design Doc → In Review → Approved
2. Implementation Plan → In Progress
3. Tasks executed → Plan updated
4. Feature complete → Status: Implemented
5. Plan archived (kept for historical reference)
```

### Referencing from code

**Link to plans in commit messages:**
```
feat(voice): implement DM voice calls

Implements DM voice call feature as designed in
docs/plans/2026-01-14-dm-voice-calls-design.md

Tasks completed:
- WebRTC peer connection setup
- Signaling via WebSocket
- UI for call controls

Relates to #123
```

**Link to plans in code comments:**
```rust
// Voice call implementation follows design from:
// docs/plans/2026-01-14-dm-voice-calls-design.md
// See section "WebRTC Signaling Flow"
```

### Searching plans

**Find plans by feature:**
```bash
ls docs/plans/*feature-name*
```

**Find recent plans:**
```bash
ls -lt docs/plans/*.md | head
```

**Search plan content:**
```bash
grep -r "search term" docs/plans/
```

### Common plan topics

**Architecture:** Permission system, rate limiting, E2EE
**Frontend:** Home view, guild management, information pages
**DevOps:** Bun migration, git workflow
**Meta:** Phase plans, agent prompts

### Best practices

**DO:**
- Write design doc before implementation
- Keep plans updated as implementation progresses
- Link between design and implementation docs
- Include rationale for decisions
- Document security and performance considerations

**DON'T:**
- Modify historical plans (add notes instead)
- Skip design phase for complex features
- Leave status fields outdated
- Forget to link to actual code changes

### Integration with Sisyphus

Plans in this directory can be used by Sisyphus orchestration:

```bash
# Execute a plan
sisyphus execute docs/plans/2026-01-XX-feature-implementation.md
```

Plans should be atomic, testable, and have clear success criteria.

### Archival

Completed plans remain in this directory as historical record. They document:
- Why decisions were made
- What alternatives were considered
- How features were implemented
- Evolution of the codebase

Don't delete old plans — they're valuable context for future changes.
