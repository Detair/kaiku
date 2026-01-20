# VoiceChat Platform â€“ Projekt-Personas

Dieses Dokument definiert die Personas, die bei Design-Entscheidungen, Code-Reviews und Feature-Diskussionen als Perspektiven herangezogen werden. Jede Persona reprÃ¤sentiert eine wichtige Stakeholder-Sicht auf das Projekt.

---

## Ãœbersicht

| Persona | Rolle | Fokus | Kernfrage |
|---------|-------|-------|-----------|
| **Elrond** | Software Architect | Systemdesign, Erweiterbarkeit | â€žSkaliert das?" |
| **Ã‰owyn** | Senior Fullstack Dev | Code-QualitÃ¤t, UX | â€žIst das wartbar?" |
| **Samweis** | DevOps Engineer | Deployment, Ops | â€žLÃ¤uft das zuverlÃ¤ssig?" |
| **Faramir** | Security Engineer | Angriffsvektoren, Crypto | â€žWie kann das gehackt werden?" |
| **Gimli** | Compliance Specialist | Lizenzen, Legal | â€žIst das lizenzkonform?" |
| **Legolas** | QA Engineer | Testing, Edge-Cases | â€žIst das getestet?" |
| **Pippin** | Community Manager | User Experience | â€žVerstehen Nutzer das?" |
| **Bilbo** | Self-Hoster | Installation, Docs | â€žKann ich das einrichten?" |
| **Gandalf** | Performance Engineer | Latenz, Profiling | â€žWie schnell ist das wirklich?" |

---

## 1. Elrond â€“ Software Architect

**Hintergrund:** 12 Jahre Erfahrung, davon 4 Jahre mit Rust. Hat zuvor an einem Video-Streaming-Dienst gearbeitet. Denkt in Systemen und Abstraktionen. Hat schon viele Technologien kommen und gehen sehen.

**Perspektive:** Sieht das groÃŸe Ganze, achtet auf Erweiterbarkeit und saubere Schnittstellen. Ist pragmatisch â€“ will kein Over-Engineering, aber auch keine technischen Schulden von Anfang an. Plant fÃ¼r Jahrzehnte, nicht fÃ¼r Sprints.

**Typische Fragen:**

- â€žWie skaliert das, wenn wir spÃ¤ter doch Multi-Node brauchen?"
- â€žIst die Service-Grenze hier richtig gezogen oder schaffen wir uns zirkulÃ¤re Dependencies?"
- â€žKÃ¶nnen wir das Interface so gestalten, dass MLS spÃ¤ter ein Drop-in-Replacement ist?"
- â€žIch habe diese Architektur schon einmal scheitern sehen â€“ was machen wir anders?"

**Mantra:** *â€žDie beste Architektur ist die, die man in 2 Jahren noch verstehen und Ã¤ndern kann."*

**Review-Fokus:**

- API-Design und Schnittstellen
- Modul-Grenzen und AbhÃ¤ngigkeiten
- Erweiterbarkeit und Zukunftssicherheit
- Trade-offs zwischen KomplexitÃ¤t und FlexibilitÃ¤t

---

## 2. Ã‰owyn â€“ Senior Fullstack Developer

**Hintergrund:** 7 Jahre Erfahrung, TypeScript-Expertin, lernt gerade Rust. Hat bei einem Gaming-Startup gearbeitet und kennt die Schmerzpunkte von Discord aus Nutzersicht. UnterschÃ¤tzt man leicht â€“ zu Unrecht.

**Perspektive:** BrÃ¼cke zwischen Backend und Frontend. Denkt an Developer Experience und User Experience gleichzeitig. Will, dass der Code lesbar und wartbar bleibt. Scheut sich nicht, auch Backend-Aufgaben zu Ã¼bernehmen.

**Typische Fragen:**

- â€žWie fÃ¼hlt sich die Latenz beim Tippen im Chat an?"
- â€žSind die Tauri-Commands gut strukturiert oder wird das Frontend zum Chaos?"
- â€žKÃ¶nnen wir hier einen optimistischen UI-Update machen?"
- â€žWarum muss das so kompliziert sein? Geht das nicht einfacher?"

**Mantra:** *â€žWenn ich den Code in 6 Monaten nicht mehr verstehe, ist er falsch."*

**Review-Fokus:**

- Code-Lesbarkeit und Wartbarkeit
- Frontend-Backend-Interaktion
- Error-Handling und User-Feedback
- TypeScript-Typisierung und Rust-API-Ergonomie

---

## 3. Samweis â€“ DevOps / Infrastructure Engineer

**Hintergrund:** 9 Jahre Erfahrung, kommt aus der Linux-Welt. Betreibt selbst einen Homelab-Cluster. Liebt Docker, hasst â€žes funktioniert auf meinem Rechner". Gibt nicht auf, bis es lÃ¤uft.

**Perspektive:** Denkt an Deployment, Monitoring, Backups und was passiert, wenn nachts um 3 Uhr der Server brennt. Will, dass Self-Hoster eine gute Erfahrung haben. KÃ¼mmert sich um die Dinge, die andere vergessen.

**Typische Fragen:**

- â€žWie sieht das docker-compose fÃ¼r einen Nicht-Techniker aus?"
- â€žWas passiert, wenn PostgreSQL voll lÃ¤uft?"
- â€žHaben wir Health-Checks und vernÃ¼nftige Logs?"
- â€žWie migrieren wir die Datenbank bei Updates?"
- â€žIch trag das Backup schon, keine Sorge."

**Mantra:** *â€žWenn es nicht automatisiert ist, existiert es nicht."*

**Review-Fokus:**

- Docker-Konfiguration und Compose-Files
- Logging und Monitoring
- Backup- und Recovery-Prozesse
- Migrations- und Update-Strategien
- Ressourcen-Limits und Health-Checks

---

## 4. Faramir â€“ Cyber Security Engineer

**Hintergrund:** 10 Jahre Security, Pentesting-Background, hat CVEs in bekannter Software gefunden. Geht davon aus, dass alles gehackt werden kann und wird. Vorsichtig, aber nicht paranoid â€“ wÃ¤gt Risiken ab.

**Perspektive:** Der skeptische Advocatus Diaboli. Sucht aktiv nach Schwachstellen. Fragt immer: â€žWas, wenn ein Angreifer X tut?" Sieht E2EE nicht als Allheilmittel. Wird oft ignoriert, behÃ¤lt aber meistens recht.

**Typische Fragen/Bedenken:**

- â€žDTLS-SRTP heiÃŸt, der Server sieht Audio â€“ ist das den Nutzern klar?"
- â€žWie schÃ¼tzen wir die One-Time-Prekeys vor Depletion-Attacken?"
- â€žWas passiert bei Key Compromise? Wie ist der Recovery-Prozess?"
- â€žRate-Limiting auf Login ist gut, aber was ist mit WebSocket-Flooding?"
- â€žDer JWT ist 15 Minuten gÃ¼ltig â€“ was wenn er geleakt wird?"
- â€žIch wÃ¼rde das nicht so bauen. Aber ich werde es verteidigen, wenn ihr es tut."

**Mantra:** *â€žSicherheit ist kein Feature, das man spÃ¤ter hinzufÃ¼gt."*

**Review-Fokus:**

- Authentifizierung und Autorisierung
- Input-Validierung und Injection-PrÃ¤vention
- Kryptografische Implementierungen
- Rate-Limiting und DoS-Schutz
- Secrets-Management und Key-Rotation

---

## 5. Gimli â€“ Compliance & Licensing Specialist

**Hintergrund:** Juristischer Background mit Tech-Fokus. Arbeitet seit 6 Jahren an Open-Source-Compliance. Hat schon GPL-VerstÃ¶ÃŸe in Unternehmen aufgedeckt. Stur, wenn es um Regeln geht â€“ aber loyal.

**Perspektive:** Paranoid bezÃ¼glich Lizenzen. WeiÃŸ, dass ein einziger AGPL-Import das ganze Projekt infizieren kann. Liest jeden `Cargo.toml`-Eintrag. Versteht keinen SpaÃŸ bei Lizenzfragen.

**Typische Fragen:**

- â€žIst libsignal wirklich komplett raus? Auch in transitiven Dependencies?"
- â€žWas steht in der NOTICE-Datei von ring? MÃ¼ssen wir das dokumentieren?"
- â€žWenn jemand einen Fork macht und MongoDB anbindet, was passiert dann lizenzrechtlich?"
- â€žHaben wir cargo-deny in der CI?"
- â€žDas steht so im Vertrag. Und an VertrÃ¤ge hÃ¤lt man sich."

**Mantra:** *â€žEine vergessene Lizenz ist eine tickende Zeitbombe."*

**Review-Fokus:**

- Neue Dependencies und deren Lizenzen
- Transitive AbhÃ¤ngigkeiten
- THIRD_PARTY_NOTICES.md AktualitÃ¤t
- cargo-deny Konfiguration
- Attribution und Copyright-Header

---

## 6. Legolas â€“ Quality Assurance Engineer

**Hintergrund:** 8 Jahre QA, davon 3 Jahre in Real-Time-Systemen. Hat ein HÃ¤ndchen dafÃ¼r, Edge-Cases zu finden, an die niemand gedacht hat. Sieht Bugs, bevor sie entstehen.

**Perspektive:** Denkt in Testszenarien und User-Flows. Fragt: â€žWas passiert, wenn..." Interessiert sich fÃ¼r Reproduzierbarkeit und Testautomatisierung. PrÃ¤zise und detailorientiert.

**Typische Fragen:**

- â€žWie testen wir Voice-QualitÃ¤t automatisiert?"
- â€žWas passiert, wenn ein User wÃ¤hrend des Sprechens die Verbindung verliert?"
- â€žKÃ¶nnen wir E2EE-Flows testen ohne die Crypto zu mocken?"
- â€žWie simulieren wir 50 gleichzeitige Voice-User?"
- â€žWas ist die Test-Strategie fÃ¼r SSO mit verschiedenen Providern?"
- â€žDa war etwas. Im dritten Request. Habt ihr das auch gesehen?"

**Mantra:** *â€žWenn es keinen Test gibt, ist es kaputt â€“ wir wissen es nur noch nicht."*

**Review-Fokus:**

- Test-Coverage und Test-QualitÃ¤t
- Edge-Cases und Fehlerszenarien
- Integration-Tests und E2E-Tests
- Testbarkeit des Codes
- Reproduzierbarkeit von Bugs

---

## 7. Pippin â€“ Community Manager / Early Adopter

**Hintergrund:** Enthusiastischer Gamer, moderiert mehrere Discord-Server. Kein Entwickler, aber technisch interessiert. ReprÃ¤sentiert die Zielgruppe. Fragt Dinge, die Entwickler fÃ¼r selbstverstÃ¤ndlich halten.

**Perspektive:** Die Stimme der Nutzer. Testet Features aus User-Sicht. Gibt ehrliches Feedback, auch wenn es wehtut. Findet UX-Probleme durch Ausprobieren. Manchmal chaotisch, aber bringt frischen Wind.

**Typische Fragen:**

- â€žWarum muss ich hier dreimal klicken? Bei Discord geht das mit einem."
- â€žWas bedeutet â€šDTLS-SRTP Handshake fehlgeschlagen'? Das sagt mir nichts."
- â€žKann ich meine Freunde einladen, ohne dass sie IT studiert haben?"
- â€žDie Emojis sind zu klein. Das ist wichtig, glaubt mir."
- â€žOh, was macht dieser Knopf?"

**Mantra:** *â€žWenn ich es nicht verstehe, versteht es niemand in meiner Community."*

**Review-Fokus:**

- Fehlermeldungen und deren VerstÃ¤ndlichkeit
- Onboarding-Flow fÃ¼r neue Nutzer
- Feature-Discoverability
- Vergleich mit Discord/TeamSpeak/Mumble
- Community-relevante Features (Emojis, Mentions, etc.)

---

## 8. Bilbo â€“ Self-Hoster Enthusiast

**Hintergrund:** Technisch versiert, aber kein Entwickler. Betreibt zu Hause einen kleinen Server mit Nextcloud und Pi-hole. Will Kontrolle Ã¼ber seine Daten. Abenteuerlustig, aber schÃ¤tzt gute Dokumentation.

**Perspektive:** Testet die Installations-Dokumentation. ReprÃ¤sentiert den typischen Self-Hoster: motiviert, aber begrenzte Zeit und Geduld. Wenn Bilbo es installieren kann, kann es jeder.

**Typische Fragen:**

- â€žSteht irgendwo, welche Ports ich freigeben muss?"
- â€žWas bedeutet â€šOIDC_ISSUER_URL'? Brauche ich das?"
- â€žKann ich das auch ohne Docker installieren?"
- â€žWas mache ich, wenn das Update schiefgeht?"
- â€žDas mit dem Backup â€“ muss das sein, oder ist das optional?"
- â€žEin Abenteuer! Aber bitte mit Anleitung."

**Mantra:** *â€žIch will es selbst hosten, nicht selbst debuggen."*

**Review-Fokus:**

- README und Installations-Dokumentation
- docker-compose.yml VerstÃ¤ndlichkeit
- Umgebungsvariablen und deren Dokumentation
- Troubleshooting-Guides
- Upgrade-Dokumentation

---

## 9. Gandalf â€“ Performance Engineer

**Hintergrund:** 15 Jahre Erfahrung, hat an Low-Latency-Systemen gearbeitet (BÃ¶rsenhandel, Gaming-Server). Versteht, was auf CPU-Cycle-Ebene passiert. Kommt genau dann, wenn man ihn braucht.

**Perspektive:** Fokus auf Latenz-Optimierung, Profiling, Memory-Leaks. WeiÃŸ, dass Performance-Probleme meist architektonische Ursachen haben. Misst alles, vermutet nichts.

**Typische Fragen:**

- â€žWarum allokieren wir hier bei jedem Frame neu?"
- â€žHaben wir Flame-Graphs vom Voice-Path?"
- â€žWas ist die P99-Latenz unter Last?"
- â€žDieser Lock hier â€“ wie lange wird der gehalten?"
- â€ž50ms ist zu viel. 20ms ist akzeptabel. 10ms ist das Ziel."
- â€žEin Performance-Problem ist nie zu spÃ¤t erkannt â€“ nur zu spÃ¤t behoben."

**Mantra:** *â€žPremature optimization ist das Problem. Aber mature optimization ist die LÃ¶sung."*

**Review-Fokus:**

- Hot-Paths und deren Optimierung
- Allokationen und Memory-Management
- Lock-Contention und Concurrency
- Benchmarks und Performance-Tests
- Profiling-Ergebnisse und Flame-Graphs

---

## Verwendung der Personas

### In Design-Diskussionen

Bei neuen Features oder Architektur-Entscheidungen sollten folgende Fragen gestellt werden:

1. **Elrond:** Passt das in die Gesamtarchitektur?
2. **Faramir:** Welche Sicherheitsrisiken entstehen?
3. **Gimli:** Gibt es Lizenzprobleme?
4. **Gandalf:** Welche Performance-Implikationen hat das?

### In Code-Reviews

Je nach Art der Ã„nderung sollten verschiedene Personas priorisiert werden:

| Art der Ã„nderung | PrimÃ¤re Personas |
|------------------|------------------|
| Neue Dependency | Gimli, Faramir |
| API-Ã„nderung | Elrond, Ã‰owyn |
| Performance-kritischer Code | Gandalf, Legolas |
| UI/UX-Ã„nderung | Pippin, Ã‰owyn |
| Deployment/Config | Samweis, Bilbo |
| Sicherheitsrelevant | Faramir, Legolas |

### In der Dokumentation

- **README.md:** Bilbo-Perspektive (Self-Hoster)
- **ARCHITECTURE.md:** Elrond-Perspektive (Architektur)
- **SECURITY.md:** Faramir-Perspektive (Security)
- **CONTRIBUTING.md:** Ã‰owyn-Perspektive (Developer)

---

## Persona-Checkliste fÃ¼r PRs

```markdown
## Persona-Check

- [ ] **Elrond:** Architektur-Impact geprÃ¼ft?
- [ ] **Ã‰owyn:** Code lesbar und wartbar?
- [ ] **Samweis:** Deployment-Impact bedacht?
- [ ] **Faramir:** Security-Implikationen geprÃ¼ft?
- [ ] **Gimli:** Neue Dependencies lizenzkonform?
- [ ] **Legolas:** Tests vorhanden und sinnvoll?
- [ ] **Pippin:** UX-Impact fÃ¼r Endnutzer?
- [ ] **Bilbo:** Dokumentation aktualisiert?
- [ ] **Gandalf:** Performance-kritische Pfade geprÃ¼ft?
```

---

## Referenzen

- [PROJECT_SPEC.md](../project/specification.md) â€“ Projektanforderungen
- [ARCHITECTURE.md](../architecture/overview.md) â€“ Technische Architektur
- [STANDARDS.md](../development/standards.md) â€“ Verwendete Standards
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) â€“ LizenzprÃ¼fung
