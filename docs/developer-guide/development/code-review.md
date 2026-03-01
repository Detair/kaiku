# Code Review System

Code Reviews verwenden 8 Concern Areas mit strukturiertem Output. Für tiefere Exploration stehen 5 Characters zur Verfügung.

**Standards-Hierarchie:** Industrie-Standards → Rust-Ecosystem → Projekt-spezifisch

## Review Output Format

Jedes Review produziert einen strukturierten Report:

```markdown
# Code Review: [scope/PR title]

## 🔒 Security
- 🔴 **CRITICAL:** [issue] — file:line
- 🟡 **WARNING:** [issue] — file:line
- 🟢 **NOTE:** [issue] — file:line

## 🏗️ Architecture
...

## 📡 API Design
...

## ⚡ Performance
...

## 🛡️ Reliability
...

## 📝 Code Quality
...

## 🧪 Testing
...

## 📜 Compliance
...

---

## Summary

| Concern | Status | Issues |
|---------|--------|--------|
| Security | 🔴/🟡/🟢/✅ | count |
| ... | ... | ... |

**Verdict:** [Blocker benennen oder "Ready to merge"]
```

**Severity:**
- 🔴 **CRITICAL** — Muss vor Merge gefixt werden
- 🟡 **WARNING** — Sollte vor Merge adressiert werden
- 🟢 **NOTE** — Verbesserungsvorschlag für später
- ✅ — Keine Issues

Leere Sections zeigen "(no issues)" oder werden weggelassen.

---

## Concern Areas

### 🔒 Security

**Scope:** Authentication, Authorization, Cryptography, Input-Validierung, Secrets, Rate-Limiting, Threat Vectors

**Severity-Kriterien:**
- 🔴 CRITICAL: Exploitable Vulnerability (Injection, Auth-Bypass, Key-Exposure, fehlendes Rate-Limit auf kritischem Endpoint)
- 🟡 WARNING: Schwaches Pattern das exploitable werden könnte (fehlende Validierung, hardcoded Config)
- 🟢 NOTE: Defense-in-Depth Vorschlag

**Standards:** OWASP Top 10, CWE, E2EE-Constraints (vodozemac, DTLS-SRTP), Argon2id, JWT 15min Expiry, Rate-Limits (Login, WebSocket, API)

---

### 🏗️ Architecture

**Scope:** Service-Grenzen, Modul-Dependencies, Interface-Design, Erweiterbarkeit, Patterns

**Severity-Kriterien:**
- 🔴 CRITICAL: Bricht bestehende Contracts, erzeugt unrecoverable Tech-Debt
- 🟡 WARNING: Coupling das zukünftige Änderungen erschwert, unklare Grenzen
- 🟢 NOTE: Alternatives Pattern zur Überlegung

**Standards:** ARCHITECTURE.md, Clean Architecture, Rust-Modul-Conventions, "MLS-Drop-in-Test" (können wir MLS später einfach einbauen?)

---

### 📡 API Design

**Scope:** REST/WebSocket-Contracts, Error-Responses, Versioning, Backwards-Compatibility, Dokumentation

**Severity-Kriterien:**
- 🔴 CRITICAL: Breaking Change ohne Version-Bump
- 🟡 WARNING: Inkonsistentes Naming, fehlende Error-Codes, unklarer Contract
- 🟢 NOTE: Ergonomie-Verbesserung

**Standards:** OpenAPI-Conventions, konsistentes Error-Envelope, WebSocket-Protokoll-Spec

---

### ⚡ Performance

**Scope:** Latenz, Allocations, Lock-Contention, Memory-Leaks, Hot-Paths

**Severity-Kriterien:**
- 🔴 CRITICAL: Verletzt <50ms Voice-Latenz-Target, unbeschränktes Wachstum
- 🟡 WARNING: Allocation in Hot-Path, potenzielle Contention
- 🟢 NOTE: Optimierungs-Möglichkeit

**Standards:** Latenz-Ziele (10ms Ziel, 20ms akzeptabel, 50ms Maximum), Rust Zero-Copy Patterns, Tokio Best Practices

---

### 🛡️ Reliability

**Scope:** Error-Handling, Error-Propagation, Recovery-Strategien, Observability (Logs/Metrics/Traces), Health-Checks, Graceful Degradation

**Severity-Kriterien:**
- 🔴 CRITICAL: Silent Failure, unbehandelter Error-Path, keine Recovery möglich
- 🟡 WARNING: Fehlender Error-Context, kein strukturiertes Logging, unklarer Failure-Mode
- 🟢 NOTE: Bessere Observability Vorschlag

**Standards:** 12-Factor App (Logs as Streams), `thiserror` für Libraries / `anyhow` für Apps, `tracing` mit strukturierten Fields, Health-Endpoints

---

### 📝 Code Quality

**Scope:** Lesbarkeit, Idiomatisches Rust, Wartbarkeit, Naming, Dokumentation wo non-obvious

**Severity-Kriterien:**
- 🔴 CRITICAL: Fundamental falsches Pattern (z.B. Blocking in Async-Context)
- 🟡 WARNING: In 6 Monaten schwer verständlich, non-idiomatisch, unklare Intent
- 🟢 NOTE: Minor Style-Improvement, DRY-Opportunity

**Standards:** Rust API Guidelines, Clippy Lints, Code-Stil in CLAUDE.md, "6-Monate-Test" (verstehe ich das noch?)

---

### 🧪 Testing

**Scope:** Coverage, Edge-Cases, Failure-Szenarien, Test-Struktur, Mocking-Strategie

**Severity-Kriterien:**
- 🔴 CRITICAL: Kein Test für kritischen Path, Test der Regressions nicht catchen kann
- 🟡 WARNING: Fehlender Edge-Case (Disconnect, Timeout, Race-Condition), Brittle Test
- 🟢 NOTE: Test-Organisation Verbesserung

**Standards:** Testing Pyramid (Unit > Integration > E2E), Property-Based Testing für Parser/Protokolle, kein Mocking von Crypto

---

### 📜 Compliance

**Scope:** Lizenz-Kompatibilität, Attribution, Transitive Dependencies

**Severity-Kriterien:**
- 🔴 CRITICAL: Verbotene Lizenz (GPL, AGPL, LGPL static)
- 🟡 WARNING: Fehlende Attribution, unklare Lizenz, neue Dependency nicht in LICENSE_COMPLIANCE.md
- 🟢 NOTE: Attribution-Formatierung

**Standards:** Erlaubte/Verbotene Lizenz-Listen, cargo-deny, THIRD_PARTY_NOTICES.md

---

## Review Invocation

### Standard Review (alle 8 Concerns)

```
Review this PR
Review the changes in src/auth/
Review my last commit
```

### Scoped Review (schneller, fokussiert)

```
Review src/api/channels.rs for API design and security only
Security review the auth module
Performance review the voice hot path
```

**Wann Scoped Reviews nutzen:**
- Kleine Änderungen (<50 LOC): Security + Code Quality
- Frontend-only: Code Quality + Testing (skip Compliance, Performance)
- Docs-only: Skip alle außer Code Quality
- Neue Dependency: Compliance + Security
- Hot-Path Änderung: Performance + Reliability

---

## Character Deep-Dives

Characters sind **nicht** Teil von Standard-Reviews. Sie sind für explorative Gespräche wenn du eine bestimmte Denkweise brauchst.

### Faramir — Skeptischer Angreifer

**Mindset:** "Alles kann gehackt werden. Wie würde ich das brechen?"

**Nutze für:** Threat Modeling, Auth-Flows, Crypto-Entscheidungen

**Beispiel-Prompts:**
- "Ask Faramir about the token refresh flow"
- "Faramir, wie würdest du diese WebSocket-Auth angreifen?"
- "Was hält Faramir von unserem Key-Rotation-Prozess?"

---

### Elrond — Langzeit-Denker

**Mindset:** "Funktioniert das noch in 2 Jahren? Können wir es dann noch ändern?"

**Nutze für:** Architektur-Entscheidungen, Interface-Design, Service-Grenzen

**Beispiel-Prompts:**
- "Ask Elrond about splitting this into two services"
- "Elrond, ist dieses Interface MLS-ready?"
- "Was denkt Elrond über diese Modul-Struktur?"

---

### Gandalf — Performance-Obsessiver

**Mindset:** "Was passiert auf CPU-Cycle-Ebene? Wo sind die Allocations?"

**Nutze für:** Profiling-Strategie, Latenz-Deep-Dives, Hot-Path-Analyse

**Beispiel-Prompts:**
- "Get Gandalf to look at this allocation pattern"
- "Gandalf, wie profilen wir den Voice-Path?"
- "Was sagt Gandalf zur Lock-Contention hier?"

---

### Éowyn — Pragmatische Warterin

**Mindset:** "Verstehe ich das in 6 Monaten noch? Geht das einfacher?"

**Nutze für:** Lesbarkeits-Debatten, "Ist das zu clever?", Refactoring-Entscheidungen

**Beispiel-Prompts:**
- "Ask Éowyn if this abstraction is worth it"
- "Éowyn, ist dieser Code zu clever?"
- "Was würde Éowyn hier vereinfachen?"

---

### Pippin — Nicht-technischer User

**Mindset:** "Verstehen meine Freunde das ohne IT-Studium?"

**Nutze für:** UX-Sanity-Check, Fehlermeldungen, Feature-Discoverability

**Beispiel-Prompts:**
- "Ask Pippin about this error message"
- "Pippin, wie viele Klicks braucht das?"
- "Würde Pippins Gaming-Community das verstehen?"

---

## Workflows

### PR Description Hygiene

Vor dem Öffnen eines PRs:

1. Summary gegen Diff prüfen (stimmt Scope mit den geänderten Dateien überein?)
2. Server/API/DB/Config-Änderungen explizit im PR-Text nennen
3. User-visible Änderungen klar benennen
4. `CHANGELOG.md` unter `[Unreleased]` aktualisieren (oder im PR begründen, warum nicht nötig)

### Neue Dependency hinzufügen

1. Lizenz prüfen (Compliance-Concern)
2. `cargo deny check licenses` ausführen
3. Transitive Dependencies prüfen
4. In LICENSE_COMPLIANCE.md dokumentieren
5. THIRD_PARTY_NOTICES.md aktualisieren falls nötig
6. Security-Review für neue Dependency

### Feature-Entwicklung

1. Design-Phase: `Ask Elrond` für Architektur
2. Security-Check: `Ask Faramir` für Threat-Model
3. Implementation mit Code-Quality Standards
4. Testing nach Testing-Concern Kriterien
5. Review: `Review this PR`
6. UX-Check: `Ask Pippin` bei User-facing Features
7. Performance: `Ask Gandalf` bei Hot-Paths
8. **Changelog:** Update `CHANGELOG.md` unter `[Unreleased]`
