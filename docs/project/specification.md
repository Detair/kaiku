# VoiceChat Platform - Projektspezifikation

## ProjektÃ¼bersicht

Eine selbst-gehostete Voice- und Text-Chat-Plattform fÃ¼r Gaming-Communities, optimiert fÃ¼r niedrige Latenz, hohe SprachqualitÃ¤t und maximale Sicherheit.

**Projektstatus:** Planungsphase
**Lizenz:** MIT OR Apache-2.0 (Dual License)
**Zielgruppe:** Gaming-Communities, Self-Hoster, Organisationen mit Datenschutzanforderungen

---

## Kernziele

| PrioritÃ¤t | Ziel | Beschreibung |
|-----------|------|--------------|
| ðŸ”´ Hoch | Niedrige Latenz | Voice-Chat muss fÃ¼r Gaming geeignet sein (<50ms) |
| ðŸ”´ Hoch | Hohe SprachqualitÃ¤t | Klare VerstÃ¤ndigung auch bei vielen Teilnehmern |
| ðŸ”´ Hoch | Sicherheit | Ende-zu-Ende-VerschlÃ¼sselung, sichere Server-Architektur |
| ðŸ”´ Hoch | Geringer Ressourcenverbrauch | Client darf Gaming-Performance nicht beeintrÃ¤chtigen |
| ðŸŸ¡ Mittel | Self-Hosted First | Einfaches Deployment mit Docker |
| ðŸŸ¡ Mittel | ModularitÃ¤t | Erweiterbar durch Themes und Plugins |
| ðŸŸ¢ Niedrig | Mobile Clients | Android/iOS als Bonus spÃ¤ter |
| ðŸŸ¢ Niedrig | SaaS-Option | Architektur soll SaaS spÃ¤ter ermÃ¶glichen |

---

## Funktionale Anforderungen

### Voice-Chat

| Feature | PrioritÃ¤t | MVP | Details |
|---------|-----------|-----|---------|
| Echtzeit-Voice | ðŸ”´ Hoch | âœ… | WebRTC-basiert, Opus-Codec |
| Push-to-Talk | ðŸ”´ Hoch | âœ… | Konfigurierbare Hotkeys |
| Voice-Aktivierung | ðŸ”´ Hoch | âœ… | Einstellbare Schwellwerte |
| Noise Cancellation | ðŸ”´ Hoch | âœ… | RNNoise-basiert |
| Echo Cancellation | ðŸ”´ Hoch | âœ… | WebRTC AEC |
| LautstÃ¤rkeregelung | ðŸŸ¡ Mittel | âœ… | Pro User einstellbar |
| RÃ¤umliches Audio | ðŸŸ¢ Niedrig | âŒ | FÃ¼r Gaming interessant, spÃ¤ter |

### Text-Chat

| Feature | PrioritÃ¤t | MVP | Details |
|---------|-----------|-----|---------|
| Text-Channels | ðŸ”´ Hoch | âœ… | Separate Channels wie Discord/Slack |
| In-Voice-Chat | ðŸ”´ Hoch | âœ… | Text-Chat innerhalb von Voice-Channels |
| Markdown-Support | ðŸ”´ Hoch | âœ… | CommonMark-Standard |
| Emojis | ðŸ”´ Hoch | âœ… | Unicode + Custom Emojis |
| @Mentions | ðŸ”´ Hoch | âœ… | User und Channel Mentions |
| Bilder hochladen | ðŸ”´ Hoch | âœ… | Mit Vorschau |
| Link-Previews | ðŸŸ¡ Mittel | âœ… | Open Graph Meta-Tags |
| Datei-Uploads | ðŸŸ¡ Mittel | âœ… | Konfigurierbare Limits |
| Nachrichtenhistorie | ðŸ”´ Hoch | âœ… | Durchsuchbar |
| Nachricht bearbeiten | ðŸŸ¡ Mittel | âœ… | Mit Bearbeitungs-Indikator |
| Nachricht lÃ¶schen | ðŸŸ¡ Mittel | âœ… | Soft-Delete |
| Threads | ðŸŸ¢ Niedrig | âŒ | SpÃ¤ter |
| Reaktionen | ðŸŸ¢ Niedrig | âŒ | SpÃ¤ter |

### User-Management

| Feature | PrioritÃ¤t | MVP | Details |
|---------|-----------|-----|---------|
| Lokale User | ðŸ”´ Hoch | âœ… | Username + Passwort |
| SSO/OIDC | ðŸ”´ Hoch | âœ… | Authentik, Keycloak, Azure AD, etc. |
| MFA (TOTP) | ðŸ”´ Hoch | âœ… | Google Authenticator kompatibel |
| MFA (WebAuthn) | ðŸŸ¡ Mittel | âŒ | Hardware-Keys, spÃ¤ter |
| Rollen & Berechtigungen | ðŸ”´ Hoch | âœ… | Feingranular pro Channel |
| User-Profile | ðŸ”´ Hoch | âœ… | Avatar, Status, Bio |
| Online-Status | ðŸ”´ Hoch | âœ… | Online, Abwesend, BeschÃ¤ftigt, Offline |
| Freundesliste | ðŸŸ¡ Mittel | âŒ | SpÃ¤ter |
| Blockieren | ðŸŸ¡ Mittel | âœ… | User kÃ¶nnen andere blockieren |

### Server-Struktur

| Feature | PrioritÃ¤t | MVP | Details |
|---------|-----------|-----|---------|
| Mehrere Voice-Channels | ðŸ”´ Hoch | âœ… | Mit User-Limits |
| Mehrere Text-Channels | ðŸ”´ Hoch | âœ… | Kategorisierbar |
| Channel-Kategorien | ðŸŸ¡ Mittel | âœ… | Gruppierung |
| Private Channels | ðŸ”´ Hoch | âœ… | Berechtigungsbasiert |
| TemporÃ¤re Channels | ðŸŸ¢ Niedrig | âŒ | Auto-Delete wenn leer |

---

## Nicht-funktionale Anforderungen

### Performance

| Metrik | Ziel | Anmerkungen |
|--------|------|-------------|
| Voice-Latenz | <50ms | Ende-zu-Ende |
| Client RAM | <100MB | Im Idle |
| Client CPU | <5% | Bei aktivem Voice |
| Server: 100 User | 4 vCPU, 8GB RAM | Gleichzeitig in Voice |
| Startup-Zeit Client | <3s | Bis zur Nutzbarkeit |

### Skalierung

| Szenario | UnterstÃ¼tzung | Anmerkungen |
|----------|---------------|-------------|
| 10-100 User (normal) | âœ… Single Node | Standard-Deployment |
| 100-500 User (Spitze) | âœ… Single Node | Mehr Hardware |
| 500-1000+ User | âš ï¸ Cluster | Horizontale SFU-Skalierung nÃ¶tig |

**Entscheidung:** MVP fokussiert auf Single-Node bis 100 User. Architektur erlaubt spÃ¤tere Skalierung.

### Sicherheit

| Anforderung | Umsetzung |
|-------------|-----------|
| Transport-VerschlÃ¼sselung | TLS 1.3 fÃ¼r alle Verbindungen |
| Voice-VerschlÃ¼sselung (MVP) | DTLS-SRTP (WebRTC Standard) |
| Voice-VerschlÃ¼sselung (spÃ¤ter) | MLS fÃ¼r echte E2EE ("Paranoid Mode") |
| Text-VerschlÃ¼sselung | Olm/Megolm (vodozemac) |
| Passwort-Hashing | Argon2id |
| Session-Management | Opaque Tokens + Redis |
| MFA | TOTP (RFC 6238) |

### VerfÃ¼gbarkeit

| Anforderung | Ziel |
|-------------|------|
| Uptime | 99.9% (Self-Hosted abhÃ¤ngig von Betreiber) |
| Backup-Intervall | TÃ¤glich automatisch |
| Recovery Time | <30 Minuten aus Backup |
| Graceful Degradation | Text-Chat funktioniert bei Voice-Ausfall |

---

## Plattform-Support

### Server

| Plattform | Support | Anmerkungen |
|-----------|---------|-------------|
| Linux (Docker) | ðŸ”´ PrimÃ¤r | Ubuntu 22.04+ empfohlen |
| Linux (Native) | ðŸŸ¡ SekundÃ¤r | FÃ¼r fortgeschrittene User |
| Windows | âŒ | Nicht geplant |
| macOS | âŒ | Nicht geplant |

### Desktop-Clients

| Plattform | Support | Framework |
|-----------|---------|-----------|
| Windows 10/11 | ðŸ”´ PrimÃ¤r | Tauri 2.0 |
| Linux | ðŸ”´ PrimÃ¤r | Tauri 2.0 |
| macOS | ðŸ”´ PrimÃ¤r | Tauri 2.0 |

### Mobile-Clients (Bonus, spÃ¤ter)

| Plattform | Support | Framework |
|-----------|---------|-----------|
| Android | ðŸŸ¢ Bonus | Flutter oder React Native |
| iOS | ðŸŸ¢ Bonus | Flutter oder React Native |

### Web-Client

| Support | Anmerkungen |
|---------|-------------|
| ðŸŸ¡ Optional | EingeschrÃ¤nkte Features mÃ¶glich via WebRTC im Browser |

---

## Entscheidungslog

### E-001: VerschlÃ¼sselungsstrategie

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** Auswahl der VerschlÃ¼sselungsmethode fÃ¼r Voice und Text.

**Optionen:**
1. DTLS-SRTP + Signal Protocol
2. SFrame + Signal Protocol
3. MLS fÃ¼r beides
4. Mesh/P2P

**Entscheidung:** 
- MVP: DTLS-SRTP (Voice) + Olm/Megolm via vodozemac (Text)
- SpÃ¤ter optional: MLS fÃ¼r "Paranoid Mode"

**BegrÃ¼ndung:**
- Minimaler Entwicklungsaufwand fÃ¼r MVP
- Self-Hosted = Server wird vertraut
- vodozemac statt libsignal wegen AGPL-Lizenzproblem
- MLS-Architektur wird vorbereitet fÃ¼r spÃ¤teren Upgrade

---

### E-002: SSO-Handling

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** Integration von lokalen Usern und SSO-Usern.

**Entscheidung:** "Unified Identity with Local Profile"
- JIT (Just-in-Time) Provisioning fÃ¼r SSO-User
- Alle User haben lokales Profil im System
- Konfigurierbares Attribut-Mapping
- Optional: Identity Linking (lokal â†” SSO)

**BegrÃ¼ndung:**
- Einheitliche User-Behandlung im System
- FlexibilitÃ¤t fÃ¼r verschiedene SSO-Provider
- Keine manuelle User-Anlage nÃ¶tig

---

### E-003: Client-Framework

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** Auswahl des Frameworks fÃ¼r Desktop-Clients.

**Optionen:**
1. Electron
2. Tauri
3. Flutter Desktop
4. Native (Qt, GTK)

**Entscheidung:** Tauri 2.0

**BegrÃ¼ndung:**
- Deutlich geringerer RAM-Verbrauch als Electron (~80MB vs ~300MB)
- Rust-Backend passt zum Server-Stack
- Native Noise Cancellation mÃ¶glich
- Cross-Platform mit einer Codebase
- MIT/Apache 2.0 lizenziert

---

### E-004: Projekt-Lizenz

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** Wahl der Open-Source-Lizenz fÃ¼r das Projekt.

**Optionen:**
1. MIT
2. Apache 2.0
3. MIT OR Apache 2.0 (Dual)
4. GPL 3.0
5. AGPL 3.0

**Entscheidung:** MIT OR Apache-2.0 (Dual License)

**BegrÃ¼ndung:**
- Maximale KompatibilitÃ¤t
- Standard im Rust-Ã–kosystem
- Erlaubt kommerzielle Nutzung und SaaS
- Patent-Schutz durch Apache-Option
- Keine Copyleft-EinschrÃ¤nkungen

---

### E-005: Text E2EE Library

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** libsignal ist AGPL-lizenziert, was Projekt-Lizenz erzwingen wÃ¼rde.

**Optionen:**
1. libsignal (AGPL 3.0)
2. vodozemac (Apache 2.0)
3. Eigenimplementierung
4. OpenMLS (MIT)

**Entscheidung:** vodozemac

**BegrÃ¼ndung:**
- Apache 2.0 kompatibel mit Projekt-Lizenz
- Implementiert Olm (1:1) und Megolm (Gruppen)
- Production-tested durch Matrix/Element
- Pure Rust, keine C-Dependencies
- Double Ratchet mit Perfect Forward Secrecy

---

### E-006: Skalierungsstrategie

**Datum:** [Aktuelles Datum]
**Status:** Entschieden

**Kontext:** Design fÃ¼r unterschiedliche Nutzerzahlen.

**Entscheidung:**
- Phase 1 (MVP): Single-Node mit Channel-Limits (50-100 pro Voice)
- Phase 2 (bei Bedarf): Horizontale SFU-Skalierung

**BegrÃ¼ndung:**
- 99% der Use-Cases mit Single-Node abgedeckt
- Kein Over-Engineering fÃ¼r MVP
- Architektur erlaubt spÃ¤teren Upgrade

---

## Glossar

| Begriff | Definition |
|---------|------------|
| SFU | Selective Forwarding Unit - Server der Media-Streams weiterleitet |
| DTLS | Datagram TLS - VerschlÃ¼sselung fÃ¼r UDP |
| SRTP | Secure RTP - VerschlÃ¼sseltes Audio/Video-Streaming |
| MLS | Message Layer Security - Moderner E2EE-Standard fÃ¼r Gruppen |
| Olm | Double Ratchet Protokoll fÃ¼r 1:1 E2EE |
| Megolm | Effizientes Gruppen-E2EE Protokoll |
| OIDC | OpenID Connect - SSO-Standard |
| JIT | Just-in-Time - Automatische User-Erstellung beim ersten Login |
| PFS | Perfect Forward Secrecy - Kompromittierte Keys gefÃ¤hrden alte Nachrichten nicht |

---

## NÃ¤chste Schritte

1. [ ] Datenmodell finalisieren
2. [ ] API-Design (OpenAPI Spec)
3. [ ] Docker-Compose Entwicklungsumgebung
4. [ ] Projekt-Skeleton (Rust Workspace)
5. [ ] CI/CD Pipeline Setup
6. [ ] MVP Feature-Scope festlegen
7. [ ] Meilensteine und Timeline

---

## Referenzen

- [ARCHITECTURE.md](../architecture/overview.md) - Technische Architektur
- [STANDARDS.md](../development/standards.md) - Verwendete Standards und Protokolle
- [LICENSE_COMPLIANCE.md](../ops/license-compliance.md) - LizenzprÃ¼fung aller Dependencies
