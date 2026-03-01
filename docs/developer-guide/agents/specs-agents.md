<!-- Parent: ../AGENTS.md -->
# Specifications

## Purpose
Formal project specifications including architecture, standards, compliance, and UX guidelines.

## Key Files
- `ARCHITECTURE.md` - Detailed technical architecture (very comprehensive)
- `PROJECT_SPEC.md` - Project requirements and decision log
- `STANDARDS.md` - Protocols and libraries used
- `LICENSE_COMPLIANCE.md` - License audit of all dependencies
- `PERSONAS.md` - User personas and use cases
- `UX_GUIDELINES.md` - User experience guidelines
- `NOISE_REDUCTION.md` - Audio noise reduction specs

## For AI Agents

### Document Authority
These are the **source of truth** documents:
1. `ARCHITECTURE.md` - How the system is designed
2. `PROJECT_SPEC.md` - What we're building and why
3. `STANDARDS.md` - What technologies we use
4. `LICENSE_COMPLIANCE.md` - What licenses are allowed

### When Making Decisions
1. Check `ARCHITECTURE.md` for existing patterns
2. Reference `STANDARDS.md` for technology choices
3. Verify license compatibility in `LICENSE_COMPLIANCE.md`
4. Consider personas from `PERSONAS.md`

### Updating Specifications
- Major architectural changes require updating `ARCHITECTURE.md`
- New dependencies require license check and `LICENSE_COMPLIANCE.md` update
- Decision log entries go in `PROJECT_SPEC.md`

### Critical: License Constraints
From `LICENSE_COMPLIANCE.md`:
- **ALLOWED:** MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0
- **FORBIDDEN:** GPL, AGPL, LGPL (static linking)
