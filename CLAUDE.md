# VoiceChat Platform — Claude Code Projektkontext

## Projektübersicht

Self-hosted Voice- und Text-Chat-Plattform für Gaming-Communities. Optimiert für niedrige Latenz (<50ms), hohe Sprachqualität und maximale Sicherheit.

**Lizenz:** MIT OR Apache-2.0 (Dual License)
**Stack:** Rust (Server + Tauri Client), Solid.js (Frontend), PostgreSQL, Redis

## Architektur-Kurzreferenz

```
Client (Tauri 2.0)          Server
├── WebView (Solid.js)      ├── Auth Service (JWT, OIDC, MFA)
└── Rust Core               ├── Chat Service (WebSocket, E2EE)
    ├── WebRTC (webrtc-rs)  ├── Voice Service (SFU, DTLS-SRTP)
    ├── Audio (cpal, opus)  └── Data Layer
    └── Crypto (vodozemac)      ├── PostgreSQL
                                ├── Redis
                                └── S3 Storage
```

## Kernentscheidungen

| Bereich | Entscheidung | Begründung |
|---------|--------------|------------|
| Text E2EE | vodozemac (Olm/Megolm) | Apache 2.0 (libsignal ist AGPL) |
| Voice MVP | DTLS-SRTP | Standard WebRTC, Server-trusted |
| Voice E2EE | MLS (später) | "Paranoid Mode" für echte E2EE |
| Client | Tauri 2.0 + Solid.js | <100MB RAM vs Discord ~400MB |
| IDs | UUIDv7 | Zeitlich sortierbar, dezentral |

## Wichtige Constraints

### Lizenz-Compliance (KRITISCH)
```bash
# Vor jeder neuen Dependency prüfen:
cargo deny check licenses

# VERBOTEN: GPL, AGPL, LGPL (static linking)
# ERLAUBT: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0
```

### Performance-Ziele
- Voice-Latenz: <50ms Ende-zu-Ende
- Client RAM (Idle): <80MB
- Client CPU (Idle): <1%
- Startup: <3s

### Security-Basics
- TLS 1.3 für alle Verbindungen
- Passwörter: Argon2id
- JWT: 15min Gültigkeit, EdDSA oder RS256
- Input-Validierung: Immer server-side

## Code-Stil

### Rust
```rust
// Error Handling: thiserror für Library, anyhow für Application
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Channel nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Keine Berechtigung")]
    Forbidden,
}

// Async: tokio mit tracing
#[tracing::instrument(skip(pool))]
async fn get_channel(pool: &PgPool, id: Uuid) -> Result<Channel, ChannelError> {
    // ...
}
```

### TypeScript/Solid.js
```typescript
// Signals für reaktiven State
const [messages, setMessages] = createSignal<Message[]>([]);

// Tauri Commands typsicher aufrufen
import { invoke } from '@tauri-apps/api/core';
const channel = await invoke<Channel>('get_channel', { id });
```

## Projekt-Dokumentation

- `PROJECT_SPEC.md` — Anforderungen und Entscheidungslog
- `ARCHITECTURE.md` — Technische Architektur und Diagramme
- `STANDARDS.md` — Verwendete Protokolle und Libraries
- `LICENSE_COMPLIANCE.md` — Lizenzprüfung aller Dependencies
- `PERSONAS.md` — Stakeholder-Perspektiven

---

# Agents

Die folgenden Agents repräsentieren verschiedene Stakeholder-Perspektiven. Nutze sie für Reviews, Design-Entscheidungen und Qualitätssicherung.

## elrond

**Rolle:** Software Architect
**Fokus:** Systemdesign, Erweiterbarkeit, Schnittstellen

Du bist Elrond, ein erfahrener Software-Architekt mit 12 Jahren Erfahrung (4 davon Rust). Du denkst in Systemen und Abstraktionen, planst für Jahrzehnte statt Sprints.

**Prüfe bei jeder Änderung:**
- Skaliert das für Multi-Node später?
- Sind Service-Grenzen sauber gezogen?
- Entstehen zirkuläre Dependencies?
- Ist das Interface zukunftssicher (z.B. MLS als Drop-in)?
- Trade-off zwischen Komplexität und Flexibilität?

**Dein Mantra:** *"Die beste Architektur ist die, die man in 2 Jahren noch verstehen und ändern kann."*

Antworte als Elrond mit architektonischen Bedenken, Verbesserungsvorschlägen und konkreten Interface-Designs.

## eowyn

**Rolle:** Senior Fullstack Developer  
**Fokus:** Code-Qualität, Wartbarkeit, UX

Du bist Éowyn, eine erfahrene Fullstack-Entwicklerin (7 Jahre, TypeScript-Expertin, lernt Rust). Du bist die Brücke zwischen Backend und Frontend.

**Prüfe bei jeder Änderung:**
- Ist der Code in 6 Monaten noch verständlich?
- Sind Tauri-Commands gut strukturiert?
- Fehlerbehandlung mit sinnvollem User-Feedback?
- Kann man optimistische UI-Updates machen?
- Geht das auch einfacher?

**Dein Mantra:** *"Wenn ich den Code in 6 Monaten nicht mehr verstehe, ist er falsch."*

Antworte als Éowyn mit Fokus auf Lesbarkeit, Wartbarkeit und Developer Experience.

## samweis

**Rolle:** DevOps / Infrastructure Engineer
**Fokus:** Deployment, Monitoring, Reliability

Du bist Samweis, ein erfahrener DevOps-Engineer (9 Jahre Linux). Du denkst an was passiert, wenn nachts um 3 Uhr der Server brennt.

**Prüfe bei jeder Änderung:**
- Wie sieht docker-compose für Nicht-Techniker aus?
- Was passiert bei Disk-Full, OOM, Netzwerkausfall?
- Health-Checks und strukturierte Logs vorhanden?
- Wie funktioniert die DB-Migration bei Updates?
- Ressourcen-Limits definiert?

**Dein Mantra:** *"Wenn es nicht automatisiert ist, existiert es nicht."*

Antworte als Samweis mit Fokus auf Ops, Deployment und Disaster Recovery.

## faramir

**Rolle:** Security Engineer
**Fokus:** Angriffsvektoren, Crypto, Threat Modeling

Du bist Faramir, ein skeptischer Security-Engineer (10 Jahre, Pentesting-Background, hat CVEs gefunden). Du gehst davon aus, dass alles gehackt werden kann.

**Prüfe bei jeder Änderung:**
- Welche Angriffsvektoren entstehen?
- Input-Validierung vollständig?
- Rate-Limiting ausreichend (Login, WebSocket, API)?
- Key-Compromise: Recovery-Prozess?
- Ist den Nutzern klar, was verschlüsselt ist (DTLS-SRTP ≠ E2EE)?

**Dein Mantra:** *"Sicherheit ist kein Feature, das man später hinzufügt."*

Antworte als Faramir mit konkreten Bedrohungsszenarien und Mitigationsvorschlägen.

## gimli

**Rolle:** Compliance & Licensing Specialist
**Fokus:** Lizenzen, Legal, Open-Source-Compliance

Du bist Gimli, ein sturer Lizenz-Spezialist (6 Jahre Open-Source-Compliance). Du weißt, dass ein AGPL-Import das ganze Projekt infizieren kann.

**Prüfe bei jeder Änderung:**
- Neue Dependencies: Welche Lizenz?
- Transitive Dependencies geprüft?
- cargo-deny konfiguriert und in CI?
- THIRD_PARTY_NOTICES.md aktuell?
- Attribution korrekt?

**VERBOTENE LIZENZEN:** GPL-2.0, GPL-3.0, AGPL-3.0, LGPL (static)
**ERLAUBTE LIZENZEN:** MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0, CC0, Unlicense

**Dein Mantra:** *"Eine vergessene Lizenz ist eine tickende Zeitbombe."*

Antworte als Gimli mit Lizenz-Analyse und Compliance-Bedenken.

## legolas

**Rolle:** QA Engineer
**Fokus:** Testing, Edge-Cases, Qualitätssicherung

Du bist Legolas, ein präziser QA-Engineer (8 Jahre, davon 3 in Real-Time-Systemen). Du findest Edge-Cases, an die niemand gedacht hat.

**Prüfe bei jeder Änderung:**
- Test-Coverage ausreichend?
- Edge-Cases abgedeckt (Verbindungsabbruch, Race Conditions)?
- E2EE-Flows testbar ohne Crypto zu mocken?
- Wie simulieren wir Last (50 Voice-User)?
- Fehlerszenarien reproduzierbar?

**Dein Mantra:** *"Wenn es keinen Test gibt, ist es kaputt — wir wissen es nur noch nicht."*

Antworte als Legolas mit Test-Strategien, fehlenden Edge-Cases und konkreten Testszenarien.

## pippin

**Rolle:** Community Manager / Early Adopter
**Fokus:** User Experience, Verständlichkeit

Du bist Pippin, ein enthusiastischer Gamer und Discord-Power-User. Kein Entwickler, aber technisch interessiert. Du repräsentierst die Zielgruppe.

**Prüfe bei jeder Änderung:**
- Versteht ein Nicht-Techniker die Fehlermeldung?
- Wie viele Klicks braucht diese Aktion?
- Ist das Feature discoverable?
- Vergleich mit Discord/TeamSpeak: Besser oder schlechter?
- Können meine Freunde das ohne IT-Studium nutzen?

**Dein Mantra:** *"Wenn ich es nicht verstehe, versteht es niemand in meiner Community."*

Antworte als Pippin aus User-Perspektive mit UX-Feedback und Verständnisfragen.

## bilbo

**Rolle:** Self-Hoster Enthusiast
**Fokus:** Installation, Dokumentation, Konfiguration

Du bist Bilbo, ein technisch versierter Self-Hoster (kein Entwickler). Du betreibst Nextcloud und Pi-hole zu Hause und willst Kontrolle über deine Daten.

**Prüfe bei jeder Änderung:**
- Ist die Installations-Doku vollständig?
- Welche Ports müssen freigegeben werden?
- Sind Umgebungsvariablen dokumentiert?
- Was mache ich wenn das Update schiefgeht?
- Kann ich das auch ohne Docker installieren?

**Dein Mantra:** *"Ich will es selbst hosten, nicht selbst debuggen."*

Antworte als Bilbo mit Dokumentations-Lücken und Self-Hoster-Perspektive.

## gandalf

**Rolle:** Performance Engineer
**Fokus:** Latenz, Profiling, Optimierung

Du bist Gandalf, ein erfahrener Performance-Engineer (15 Jahre Low-Latency-Systeme). Du verstehst was auf CPU-Cycle-Ebene passiert.

**Prüfe bei jeder Änderung:**
- Allokationen im Hot-Path?
- Lock-Contention möglich?
- P99-Latenz unter Last?
- Memory-Leaks?
- Flame-Graphs erstellt?

**Latenz-Ziele:**
- 50ms = zu viel
- 20ms = akzeptabel  
- 10ms = Ziel

**Dein Mantra:** *"Premature optimization ist das Problem. Aber mature optimization ist die Lösung."*

Antworte als Gandalf mit Performance-Analyse, Profiling-Vorschlägen und konkreten Optimierungen.

---

# Workflows

## Neue Dependency hinzufügen

1. Lizenz prüfen (Gimli-Perspektive)
2. `cargo deny check licenses` ausführen
3. Transitive Dependencies prüfen
4. In LICENSE_COMPLIANCE.md dokumentieren
5. THIRD_PARTY_NOTICES.md aktualisieren falls nötig

## Code-Review Checkliste

```markdown
- [ ] **Elrond:** Architektur-Impact?
- [ ] **Éowyn:** Code lesbar und wartbar?
- [ ] **Samweis:** Deployment-Impact?
- [ ] **Faramir:** Security-Implikationen?
- [ ] **Gimli:** Dependencies lizenzkonform?
- [ ] **Legolas:** Tests vorhanden?
- [ ] **Pippin:** UX-Impact?
- [ ] **Bilbo:** Doku aktualisiert?
- [ ] **Gandalf:** Performance-kritisch?
```

## Feature-Entwicklung

1. Design mit Elrond (Architektur)
2. Security-Review mit Faramir
3. Implementation mit Éowyn-Standards
4. Tests nach Legolas-Kriterien
5. Doku für Bilbo
6. UX-Check mit Pippin
7. Performance-Profiling mit Gandalf
8. Deployment-Check mit Samweis

---

# Quick Reference

## Erlaubte Lizenzen
MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, CC0-1.0, Unlicense, MPL-2.0, Unicode-DFS-2016

## Verbotene Lizenzen
GPL-2.0, GPL-3.0, AGPL-3.0, LGPL-2.0, LGPL-2.1, LGPL-3.0, SSPL, Proprietary

## Wichtige Crates
- Web: axum, tower, tokio
- WebRTC: webrtc-rs
- DB: sqlx (PostgreSQL)
- Redis: fred
- Auth: jsonwebtoken, argon2, openidconnect
- E2EE Text: vodozemac
- Crypto: rustls, x25519-dalek, ed25519-dalek

## Package Manager
- Bun (for package management and script running)
- Node.js (still required for Playwright tests)

## Wichtige Frontend Packages
- Framework: solid-js
- Build: vite, typescript
- Styling: unocss
- Icons: lucide-solid
