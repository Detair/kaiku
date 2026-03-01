# Persona System Rework Design

## Problem Statement

The current 9-persona system has issues:
- Too many personas (overwhelming)
- Overlapping scopes producing non-compact feedback
- Missing coverage for API design, observability, backend error handling

## Solution Overview

**Primary:** 8 concern areas with best-practice checklists (structured review output)
**Secondary:** 5 consolidated characters for deep-dive exploration (conversational)

Standards hierarchy: Industry standards → Rust ecosystem → Project-specific rules

---

## Review Output Format

Every code review produces a single structured report:

```markdown
# Code Review: [scope/PR title]

## Security
- CRITICAL: [issue] — file:line
- WARNING: [issue] — file:line
- NOTE: [issue] — file:line

## Architecture
...

## API Design
...

## Performance
...

## Reliability
...

## Code Quality
...

## Testing
...

## Compliance
...

---

## Summary

| Concern | Status | Issues |
|---------|--------|--------|
| Security | /  | count |
| ... | ... | ... |

**Verdict:** [One-line consolidated assessment]
```

Severity indicators:
- CRITICAL — Must fix before merge
- WARNING — Should address before merge
- NOTE — Consider for future improvement

Empty sections show "(no issues)" or are omitted.

---

## Concern Area Definitions

### Security

**Scope:** Authentication, authorization, cryptography, input validation, secrets handling, threat vectors

**Severity:**
- CRITICAL: Exploitable vulnerability (injection, auth bypass, key exposure)
- WARNING: Weak pattern that could become exploitable (missing validation, hardcoded config)
- NOTE: Defense-in-depth suggestion

**Standards:** OWASP Top 10, CWE, E2EE constraints (vodozemac, DTLS-SRTP), Argon2id for passwords, JWT 15min expiry

---

### Architecture

**Scope:** Service boundaries, module dependencies, interface design, extensibility, patterns

**Severity:**
- CRITICAL: Breaks existing contracts, creates unrecoverable tech debt
- WARNING: Coupling that limits future changes, unclear boundaries
- NOTE: Alternative pattern worth considering

**Standards:** ARCHITECTURE.md, Clean Architecture principles, Rust module conventions, "can we swap MLS in later?" test

---

### API Design

**Scope:** REST/WebSocket contracts, error responses, versioning, backwards compatibility, documentation

**Severity:**
- CRITICAL: Breaking change to public API without version bump
- WARNING: Inconsistent naming, missing error codes, unclear contract
- NOTE: Ergonomic improvement

**Standards:** OpenAPI conventions, consistent error envelope, WebSocket protocol spec

---

### Performance

**Scope:** Latency, allocations, lock contention, memory leaks, hot paths

**Severity:**
- CRITICAL: Violates <50ms voice latency target, unbounded growth
- WARNING: Allocation in hot path, potential contention
- NOTE: Optimization opportunity

**Standards:** Latency targets (10ms goal, 20ms acceptable, 50ms max), Rust zero-copy patterns, tokio best practices

---

### Reliability

**Scope:** Error handling, error propagation, recovery strategies, observability (logs/metrics/traces), health checks, graceful degradation

**Severity:**
- CRITICAL: Silent failure, unhandled error path, no recovery possible
- WARNING: Missing error context, no structured logging, unclear failure mode
- NOTE: Better observability suggestion

**Standards:** 12-Factor App (logs as streams), `thiserror` for libraries / `anyhow` for apps, `tracing` with structured fields, health endpoints

---

### Code Quality

**Scope:** Readability, idiomatic Rust, maintainability, naming, documentation where non-obvious

**Severity:**
- CRITICAL: Fundamentally wrong pattern (e.g., blocking in async context)
- WARNING: Hard to understand in 6 months, non-idiomatic, unclear intent
- NOTE: Minor style improvement, DRY opportunity

**Standards:** Rust API Guidelines, Clippy lints, code style in CLAUDE.md, "readable in 6 months?" test

---

### Testing

**Scope:** Coverage, edge cases, failure scenarios, test structure, mocking strategy

**Severity:**
- CRITICAL: No test for critical path, test that can't catch regressions
- WARNING: Missing edge case (disconnect, timeout, race condition), brittle test
- NOTE: Test organization improvement

**Standards:** Testing pyramid (unit > integration > e2e), property-based testing for parsers/protocols, no mocking crypto

---

### Compliance

**Scope:** License compatibility, attribution, transitive dependencies

**Severity:**
- CRITICAL: Forbidden license (GPL, AGPL, LGPL static)
- WARNING: Missing attribution, unclear license, new dependency not in LICENSE_COMPLIANCE.md
- NOTE: Attribution formatting

**Standards:** Allowed/forbidden license lists, cargo-deny, THIRD_PARTY_NOTICES.md

---

## Character Deep-Dives

Characters are NOT part of standard reviews. They're invokable for exploratory conversations.

**Invocation:** "Ask Faramir about this auth flow"

**When to use:**
- Uncertain about a design decision and want adversarial thinking
- Need to explore trade-offs in depth
- Want to roleplay a stakeholder conversation

### Consolidated Roster (5 characters)

| Character | Mindset | Use for |
|-----------|---------|---------|
| **Faramir** | Skeptical attacker | Threat modeling, "how would I break this?" |
| **Elrond** | Long-term thinker | Architecture decisions, "will this work in 2 years?" |
| **Gandalf** | Performance obsessive | Profiling strategy, latency deep-dives |
| **Eowyn** | Pragmatic maintainer | "Is this too clever?", readability debates |
| **Pippin** | Non-technical user | UX sanity check, "would my friends understand this?" |

### Retired Characters

- Samweis → absorbed into Reliability concern
- Bilbo → absorbed into Reliability (docs) + general workflow
- Gimli → absorbed into Compliance concern
- Legolas → absorbed into Testing concern

---

## Invocation Model

### Standard Review

```
Review this PR
Review the changes in src/auth/
Review my last commit
```

Produces structured report. All 8 concerns checked.

### Scoped Review

```
Review src/api/channels.rs for API design and security only
Security review the auth module
Performance review the voice hot path
```

Only specified concerns checked. Same output format.

### Deep-Dive

```
Ask Faramir about the token refresh flow
What would Elrond think about splitting this into two services?
Get Gandalf to look at this allocation pattern
```

Conversational response in-character, not structured report.

---

## CLAUDE.md Integration

Replace the current "Code-Review Checkliste" with:

```markdown
## Code Review

Run `Review this PR` for structured report covering:
- Security, Architecture, API Design, Performance
- Reliability, Code Quality, Testing, Compliance

For deep exploration: `Ask [Faramir|Elrond|Gandalf|Eowyn|Pippin] about [topic]`
```

---

## Implementation Notes

1. Update CLAUDE.md with new concern area definitions
2. Remove verbose persona descriptions, keep 5 character summaries for deep-dives
3. Add review output format template
4. Update workflow documentation
