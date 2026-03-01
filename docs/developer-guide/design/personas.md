# Kaiku Platform – Project Personas

This document defines the personas used as perspectives in design decisions, code reviews, and feature discussions. Each persona represents an important stakeholder view on the project.

---

## Overview

| Persona | Role | Focus | Core Question |
|---------|------|-------|---------------|
| **Elrond** | Software Architect | System Design, Extensibility | "Does this scale?" |
| **Éowyn** | Senior Fullstack Dev | Code Quality, UX | "Is this maintainable?" |
| **Samweis** | DevOps Engineer | Deployment, Ops | "Does this run reliably?" |
| **Faramir** | Security Engineer | Attack Vectors, Crypto | "How can this be hacked?" |
| **Gimli** | Compliance Specialist | Licenses, Legal | "Is this license-compliant?" |
| **Legolas** | QA Engineer | Testing, Edge Cases | "Is this tested?" |
| **Pippin** | Community Manager | User Experience | "Do users understand this?" |
| **Bilbo** | Self-Hoster | Installation, Docs | "Can I set this up?" |
| **Gandalf** | Performance Engineer | Latency, Profiling | "How fast is this really?" |

---

## 1. Elrond – Software Architect

**Background:** 12 years of experience, 4 of which with Rust. Previously worked on a video streaming service. Thinks in systems and abstractions. Has seen many technologies come and go.

**Perspective:** Sees the big picture, focuses on extensibility and clean interfaces. Is pragmatic – wants no over-engineering, but also no technical debt from the start. Plans for decades, not sprints.

**Typical Questions:**

- "How does this scale if we need multi-node later?"
- "Is the service boundary drawn correctly here, or are we creating circular dependencies?"
- "Can we design this interface so MLS can be a drop-in replacement later?"
- "I've seen this architecture fail before – what are we doing differently?"

**Mantra:** *"The best architecture is one you can still understand and modify in 2 years."*

**Review Focus:**

- API design and interfaces
- Module boundaries and dependencies
- Extensibility and future-proofing
- Trade-offs between complexity and flexibility

---

## 2. Éowyn – Senior Fullstack Developer

**Background:** 7 years of experience, TypeScript expert, currently learning Rust. Worked at a gaming startup and knows Discord's pain points from a user perspective. Often underestimated – wrongly so.

**Perspective:** Bridge between backend and frontend. Thinks about developer experience and user experience simultaneously. Wants code to remain readable and maintainable. Not afraid to take on backend tasks.

**Typical Questions:**

- "How does the latency feel when typing in chat?"
- "Are the Tauri commands well-structured, or is the frontend becoming chaotic?"
- "Can we do an optimistic UI update here?"
- "Why does this have to be so complicated? Can't it be simpler?"

**Mantra:** *"If I can't understand the code in 6 months, it's wrong."*

**Review Focus:**

- Code readability and maintainability
- Frontend-backend interaction
- Error handling and user feedback
- TypeScript typing and Rust API ergonomics

---

## 3. Samweis – DevOps / Infrastructure Engineer

**Background:** 9 years of experience, comes from the Linux world. Runs a homelab cluster. Loves Docker, hates "it works on my machine". Doesn't give up until it works.

**Perspective:** Thinks about deployment, monitoring, backups, and what happens when the server catches fire at 3 AM. Wants self-hosters to have a good experience. Takes care of the things others forget.

**Typical Questions:**

- "What does the docker-compose look like for a non-technical user?"
- "What happens when PostgreSQL runs out of disk space?"
- "Do we have health checks and proper logs?"
- "How do we migrate the database on updates?"
- "I'll carry the backup, don't worry."

**Mantra:** *"If it's not automated, it doesn't exist."*

**Review Focus:**

- Docker configuration and compose files
- Logging and monitoring
- Backup and recovery processes
- Migration and update strategies
- Resource limits and health checks

---

## 4. Faramir – Cyber Security Engineer

**Background:** 10 years in security, pentesting background, has found CVEs in well-known software. Assumes everything can and will be hacked. Cautious but not paranoid – weighs risks.

**Perspective:** The skeptical devil's advocate. Actively looks for vulnerabilities. Always asks: "What if an attacker does X?" Doesn't see E2EE as a silver bullet. Often ignored, but usually right.

**Typical Questions/Concerns:**

- "DTLS-SRTP means the server sees audio – is that clear to users?"
- "How do we protect one-time prekeys from depletion attacks?"
- "What happens on key compromise? What's the recovery process?"
- "Rate limiting on login is good, but what about WebSocket flooding?"
- "The JWT is valid for 15 minutes – what if it gets leaked?"
- "I wouldn't build it this way. But I'll defend it if you do."

**Mantra:** *"Security is not a feature you add later."*

**Review Focus:**

- Authentication and authorization
- Input validation and injection prevention
- Cryptographic implementations
- Rate limiting and DoS protection
- Secrets management and key rotation

---

## 5. Gimli – Compliance & Licensing Specialist

**Background:** Legal background with tech focus. Has worked on open-source compliance for 6 years. Has uncovered GPL violations in companies. Stubborn about rules – but loyal.

**Perspective:** Paranoid about licenses. Knows that a single AGPL import can infect the entire project. Reads every `Cargo.toml` entry. Doesn't joke about licensing issues.

**Typical Questions:**

- "Is libsignal completely gone? Including transitive dependencies?"
- "What does the NOTICE file of ring say? Do we need to document that?"
- "If someone forks and connects MongoDB, what happens legally?"
- "Do we have cargo-deny in CI?"
- "That's what the contract says. And you honor contracts."

**Mantra:** *"A forgotten license is a ticking time bomb."*

**Review Focus:**

- New dependencies and their licenses
- Transitive dependencies
- THIRD_PARTY_NOTICES.md currency
- cargo-deny configuration
- Attribution and copyright headers

---

## 6. Legolas – Quality Assurance Engineer

**Background:** 8 years QA, 3 of which in real-time systems. Has a knack for finding edge cases no one thought of. Sees bugs before they happen.

**Perspective:** Thinks in test scenarios and user flows. Asks: "What happens when..." Interested in reproducibility and test automation. Precise and detail-oriented.

**Typical Questions:**

- "How do we test voice quality automatically?"
- "What happens when a user loses connection while speaking?"
- "Can we test E2EE flows without mocking crypto?"
- "How do we simulate 50 concurrent voice users?"
- "What's the test strategy for SSO with different providers?"
- "There was something. In the third request. Did you see it too?"

**Mantra:** *"If there's no test, it's broken – we just don't know it yet."*

**Review Focus:**

- Test coverage and test quality
- Edge cases and error scenarios
- Integration tests and E2E tests
- Code testability
- Bug reproducibility

---

## 7. Pippin – Community Manager / Early Adopter

**Background:** Enthusiastic gamer, moderates several Discord servers. Not a developer, but technically curious. Represents the target audience. Asks things developers take for granted.

**Perspective:** The voice of users. Tests features from a user perspective. Gives honest feedback, even when it hurts. Finds UX problems through trying things out. Sometimes chaotic, but brings fresh air.

**Typical Questions:**

- "Why do I have to click three times here? Discord does it with one."
- "What does 'DTLS-SRTP handshake failed' mean? That tells me nothing."
- "Can I invite my friends without them having an IT degree?"
- "The emojis are too small. This is important, trust me."
- "Oh, what does this button do?"

**Mantra:** *"If I don't understand it, nobody in my community will."*

**Review Focus:**

- Error messages and their clarity
- Onboarding flow for new users
- Feature discoverability
- Comparison with Discord/TeamSpeak/Mumble
- Community-relevant features (emojis, mentions, etc.)

---

## 8. Bilbo – Self-Hoster Enthusiast

**Background:** Technically savvy, but not a developer. Runs a small home server with Nextcloud and Pi-hole. Wants control over his data. Adventurous, but values good documentation.

**Perspective:** Tests the installation documentation. Represents the typical self-hoster: motivated but with limited time and patience. If Bilbo can install it, anyone can.

**Typical Questions:**

- "Does it say anywhere which ports I need to open?"
- "What does 'OIDC_ISSUER_URL' mean? Do I need that?"
- "Can I also install this without Docker?"
- "What do I do if the update goes wrong?"
- "The backup thing – is that required, or is it optional?"
- "An adventure! But please with instructions."

**Mantra:** *"I want to self-host it, not self-debug it."*

**Review Focus:**

- README and installation documentation
- docker-compose.yml clarity
- Environment variables and their documentation
- Troubleshooting guides
- Upgrade documentation

---

## 9. Gandalf – Performance Engineer

**Background:** 15 years of experience, has worked on low-latency systems (stock trading, gaming servers). Understands what happens at the CPU cycle level. Arrives exactly when needed.

**Perspective:** Focus on latency optimization, profiling, memory leaks. Knows that performance problems usually have architectural causes. Measures everything, assumes nothing.

**Typical Questions:**

- "Why are we allocating here on every frame?"
- "Do we have flame graphs from the voice path?"
- "What's the P99 latency under load?"
- "This lock here – how long is it held?"
- "50ms is too much. 20ms is acceptable. 10ms is the goal."
- "A performance problem is never detected too late – only fixed too late."

**Mantra:** *"Premature optimization is the problem. But mature optimization is the solution."*

**Review Focus:**

- Hot paths and their optimization
- Allocations and memory management
- Lock contention and concurrency
- Benchmarks and performance tests
- Profiling results and flame graphs

---

## Using the Personas

### In Design Discussions

When discussing new features or architecture decisions, the following questions should be asked:

1. **Elrond:** Does this fit the overall architecture?
2. **Faramir:** What security risks arise?
3. **Gimli:** Are there licensing issues?
4. **Gandalf:** What are the performance implications?

### In Code Reviews

Depending on the type of change, different personas should be prioritized:

| Type of Change | Primary Personas |
|----------------|------------------|
| New Dependency | Gimli, Faramir |
| API Change | Elrond, Éowyn |
| Performance-critical Code | Gandalf, Legolas |
| UI/UX Change | Pippin, Éowyn |
| Deployment/Config | Samweis, Bilbo |
| Security-relevant | Faramir, Legolas |

### In Documentation

- **README.md:** Bilbo perspective (Self-Hoster)
- **ARCHITECTURE.md:** Elrond perspective (Architecture)
- **SECURITY.md:** Faramir perspective (Security)
- **CONTRIBUTING.md:** Éowyn perspective (Developer)

---

## Persona Checklist for PRs

```markdown
## Persona Check

- [ ] **Elrond:** Architecture impact reviewed?
- [ ] **Éowyn:** Code readable and maintainable?
- [ ] **Samweis:** Deployment impact considered?
- [ ] **Faramir:** Security implications reviewed?
- [ ] **Gimli:** New dependencies license-compliant?
- [ ] **Legolas:** Tests present and meaningful?
- [ ] **Pippin:** UX impact for end users?
- [ ] **Bilbo:** Documentation updated?
- [ ] **Gandalf:** Performance-critical paths reviewed?
```

---

## References

- [PROJECT_SPEC.md](../project/specification.md) – Project Requirements
- [ARCHITECTURE.md](../architecture/overview.md) – Technical Architecture
- [STANDARDS.md](../development/standards.md) – Standards Used
- [LICENSE_COMPLIANCE.md](../../../LICENSE_COMPLIANCE.md) – License Compliance
