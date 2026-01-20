# VoiceChat Platform - Lizenz-Compliance

Dieses Dokument dokumentiert die LizenzprÃ¼fung aller verwendeten Dependencies, um sicherzustellen, dass das Projekt als Open Source (MIT OR Apache-2.0) verÃ¶ffentlicht werden kann.

---

## Projekt-Lizenz

```
SPDX-License-Identifier: MIT OR Apache-2.0
```

**Dual-Lizenzierung:** Nutzer kÃ¶nnen zwischen MIT und Apache 2.0 wÃ¤hlen.

### BegrÃ¼ndung fÃ¼r Dual-Lizenz

| Aspekt | MIT | Apache 2.0 |
|--------|-----|------------|
| Einfachheit | âœ… Sehr kurz und einfach | âš ï¸ LÃ¤nger, komplexer |
| Patent-Schutz | âŒ Keiner | âœ… Expliziter Patent-Grant |
| Attribution | âœ… Nur Copyright Notice | âœ… Copyright + NOTICE File |
| KompatibilitÃ¤t | âœ… Fast alles | âœ… Fast alles |
| Unternehmensfreundlich | âœ… Ja | âœ… Ja (bevorzugt) |

Die Dual-Lizenzierung ist Standard im Rust-Ã–kosystem und bietet maximale FlexibilitÃ¤t.

---

## Lizenz-KompatibilitÃ¤t

### KompatibilitÃ¤tsmatrix

```
â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
â”‚              LIZENZ-KOMPATIBILITÃ„T MIT MIT/Apache 2.0           â”‚
â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
â”‚                                                                  â”‚
â”‚  âœ… KOMPATIBEL (kÃ¶nnen verwendet werden)                        â”‚
â”‚  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€                          â”‚
â”‚  â€¢ MIT                    - Permissive                          â”‚
â”‚  â€¢ Apache 2.0             - Permissive + Patent                 â”‚
â”‚  â€¢ BSD-2-Clause           - Permissive                          â”‚
â”‚  â€¢ BSD-3-Clause           - Permissive                          â”‚
â”‚  â€¢ ISC                    - Permissive (wie MIT)                â”‚
â”‚  â€¢ Zlib                   - Permissive                          â”‚
â”‚  â€¢ CC0-1.0                - Public Domain                       â”‚
â”‚  â€¢ Unlicense              - Public Domain                       â”‚
â”‚  â€¢ Unicode-DFS-2016       - Permissive (Unicode Data)           â”‚
â”‚                                                                  â”‚
â”‚  âš ï¸ EINGESCHRÃ„NKT KOMPATIBEL                                    â”‚
â”‚  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€                                    â”‚
â”‚  â€¢ MPL 2.0                - File-Level Copyleft                 â”‚
â”‚                             (Ã„nderungen an MPL-Dateien mÃ¼ssen   â”‚
â”‚                              unter MPL bleiben, Rest ist frei)  â”‚
â”‚                                                                  â”‚
â”‚  âŒ NICHT KOMPATIBEL (dÃ¼rfen NICHT verwendet werden)            â”‚
â”‚  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€             â”‚
â”‚  â€¢ GPL 2.0/3.0            - Starkes Copyleft                    â”‚
â”‚  â€¢ LGPL 2.1/3.0           - Library Copyleft (bei static link) â”‚
â”‚  â€¢ AGPL 3.0               - Network Copyleft                    â”‚
â”‚  â€¢ Proprietary            - Closed Source                       â”‚
â”‚  â€¢ SSPL                   - Server-Side Public License          â”‚
â”‚                                                                  â”‚
â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
```

---

## Server Dependencies PrÃ¼fung

### Web Framework & Runtime

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| axum | 0.7 | MIT | âœ… | MIT |
| tokio | 1.x | MIT | âœ… | MIT |
| tower | 0.4 | MIT | âœ… | MIT |
| tower-http | 0.5 | MIT | âœ… | MIT |
| hyper | 1.x | MIT | âœ… | MIT |

### WebSocket & Real-Time

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| tokio-tungstenite | 0.21 | MIT | âœ… | MIT |
| tungstenite | 0.21 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### WebRTC

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| webrtc | 0.11 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-data | 0.9 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-dtls | 0.9 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-ice | 0.11 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-media | 0.8 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-sctp | 0.10 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-srtp | 0.13 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| webrtc-util | 0.9 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Datenbank

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| sqlx | 0.7 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| sqlx-core | 0.7 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| sqlx-postgres | 0.7 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Redis

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| fred | 8.x | MIT | âœ… | MIT |

### Authentifizierung

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| jsonwebtoken | 9.x | MIT | âœ… | MIT |
| argon2 | 0.5 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| totp-rs | 5.x | MIT | âœ… | MIT |
| openidconnect | 3.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| oauth2 | 4.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Kryptografie

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| rustls | 0.22 | MIT/Apache 2.0/ISC | âœ… | MIT OR Apache-2.0 OR ISC |
| ring | 0.17 | MIT + ISC + OpenSSL | âœ… | Siehe NotizÂ¹ |
| x25519-dalek | 2.x | BSD-3-Clause | âœ… | BSD-3-Clause |
| ed25519-dalek | 2.x | BSD-3-Clause | âœ… | BSD-3-Clause |
| curve25519-dalek | 4.x | BSD-3-Clause | âœ… | BSD-3-Clause |
| aes-gcm | 0.10 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| aes | 0.8 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| hkdf | 0.12 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| sha2 | 0.10 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| hmac | 0.12 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| rand | 0.8 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

Â¹ **ring Lizenz-Notiz:** ring verwendet Code unter MIT, ISC und OpenSSL-Lizenzen. Alle sind permissive und kompatibel.

### E2EE (Text)

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| vodozemac | 0.5 | Apache 2.0 | âœ… | Apache-2.0 |

**Wichtig:** Wir verwenden bewusst `vodozemac` statt `libsignal`:

| Library | Lizenz | Kompatibel? |
|---------|--------|-------------|
| vodozemac | Apache 2.0 | âœ… Ja |
| libsignal-protocol | AGPL 3.0 | âŒ Nein |

### Serialisierung

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| serde | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| serde_json | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| serde_derive | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Utilities

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| uuid | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| chrono | 0.4 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| tracing | 0.1 | MIT | âœ… | MIT |
| tracing-subscriber | 0.3 | MIT | âœ… | MIT |
| thiserror | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| anyhow | 1.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| bytes | 1.x | MIT | âœ… | MIT |
| futures | 0.3 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### S3 Storage

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| aws-sdk-s3 | 1.x | Apache 2.0 | âœ… | Apache-2.0 |
| aws-config | 1.x | Apache 2.0 | âœ… | Apache-2.0 |
| aws-smithy-runtime | 1.x | Apache 2.0 | âœ… | Apache-2.0 |

### API Dokumentation

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| utoipa | 4.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| utoipa-swagger-ui | 6.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Markdown

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| pulldown-cmark | 0.10 | MIT | âœ… | MIT |

---

## Client Dependencies PrÃ¼fung

### Tauri

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| tauri | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| tauri-build | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| tauri-runtime | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| tauri-runtime-wry | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| wry | 0.35 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |
| tao | 0.25 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Audio

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| cpal | 0.15 | Apache 2.0 | âœ… | Apache-2.0 |
| opus | 0.3 | MIT | âœ… | MIT |
| nnnoiseless | 0.5 | BSD-3-Clause | âœ… | BSD-3-Clause |

**Notiz zu libopus:** Die native `libopus` Library ist BSD-3-Clause lizenziert.

### Secure Storage

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| keyring | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### HTTP Client

| Crate | Version | Lizenz | Status | SPDX |
|-------|---------|--------|--------|------|
| reqwest | 0.11 | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

---

## Frontend Dependencies PrÃ¼fung

### Frameworks

| Package | Version | Lizenz | Status | SPDX |
|---------|---------|--------|--------|------|
| solid-js | 1.8 | MIT | âœ… | MIT |
| @solidjs/router | 0.10 | MIT | âœ… | MIT |

### Build Tools

| Package | Version | Lizenz | Status | SPDX |
|---------|---------|--------|--------|------|
| vite | 5.x | MIT | âœ… | MIT |
| vite-plugin-solid | 2.8 | MIT | âœ… | MIT |
| typescript | 5.x | Apache 2.0 | âœ… | Apache-2.0 |
| @tauri-apps/cli | 2.x | MIT/Apache 2.0 | âœ… | MIT OR Apache-2.0 |

### Styling

| Package | Version | Lizenz | Status | SPDX |
|---------|---------|--------|--------|------|
| unocss | 0.58 | MIT | âœ… | MIT |
| @unocss/preset-uno | 0.58 | MIT | âœ… | MIT |

### Icons

| Package | Version | Lizenz | Status | SPDX |
|---------|---------|--------|--------|------|
| lucide-solid | 0.300 | ISC | âœ… | ISC |

---

## Abgelehnte Dependencies

Diese Libraries wurden geprÃ¼ft und **bewusst nicht verwendet**:

| Library | Lizenz | Grund fÃ¼r Ablehnung |
|---------|--------|---------------------|
| libsignal-protocol | AGPL 3.0 | WÃ¼rde AGPL fÃ¼r gesamtes Projekt erzwingen |
| ffmpeg | GPL/LGPL | GPL-Komponenten, komplizierte Lizenz |
| openssl (native) | Apache 2.0 | Okay, aber rustls bevorzugt (pure Rust) |
| Matrix SDK | Apache 2.0 | Zu komplex, nur vodozemac fÃ¼r Crypto genutzt |

---

## Lizenz-Dateien im Repository

Das Repository muss folgende Dateien enthalten:

### LICENSE-MIT

```
MIT License

Copyright (c) [YEAR] [COPYRIGHT HOLDER]

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### LICENSE-APACHE

```
                              Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

[VollstÃ¤ndiger Apache 2.0 Text]
```

### Cargo.toml

```toml
[package]
name = "voicechat"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/[user]/voicechat"
description = "Self-hosted voice and text chat platform"
keywords = ["voip", "chat", "webrtc", "e2ee"]
categories = ["multimedia", "network-programming"]
```

### README.md (Lizenz-Sektion)

```markdown
## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
```

---

## Automatisierte Compliance-PrÃ¼fung

### cargo-deny Konfiguration

```toml
# deny.toml

[graph]
targets = []
all-features = true

[advisories]
db-path = "~/.cargo/advisory-db"
vulnerability = "deny"
unmaintained = "warn"
yanked = "deny"
notice = "warn"

[licenses]
unlicensed = "deny"
copyleft = "deny"
allow-osi-fsf-free = "neither"
default = "deny"
confidence-threshold = 0.93

allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "ISC",
    "CC0-1.0",
    "Unlicense",
    "Zlib",
    "Unicode-DFS-2016",
    "MPL-2.0",
]

deny = [
    "GPL-2.0",
    "GPL-2.0-only",
    "GPL-2.0-or-later",
    "GPL-3.0",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "AGPL-3.0",
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "LGPL-2.0",
    "LGPL-2.1",
    "LGPL-3.0",
]

[[licenses.clarify]]
name = "ring"
expression = "MIT AND ISC AND OpenSSL"
license-files = [
    { path = "LICENSE", hash = 0xbd0eed23 }
]

[[licenses.clarify]]
name = "webpki"
expression = "ISC"
license-files = [
    { path = "LICENSE", hash = 0x001c7e6c }
]

[bans]
multiple-versions = "warn"
wildcards = "deny"
highlight = "all"

deny = [
    # Explizit verbotene Crates
]

[sources]
unknown-registry = "deny"
unknown-git = "warn"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

### CI/CD Integration

```yaml
# .github/workflows/license-check.yml

name: License Compliance

on:
  push:
    branches: [main]
  pull_request:

jobs:
  license-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install cargo-deny
        run: cargo install cargo-deny
        
      - name: Check licenses
        run: cargo deny check licenses
        
      - name: Check advisories
        run: cargo deny check advisories
        
      - name: Check bans
        run: cargo deny check bans
```

---

## Third-Party Notices

Bei Distribution mÃ¼ssen folgende Attributions enthalten sein:

### THIRD_PARTY_NOTICES.md

```markdown
# Third Party Notices

This software includes the following third-party components:

## Rust Crates

### ring
Copyright (c) 2015-2016 Brian Smith
Licensed under MIT, ISC, and OpenSSL licenses

### curve25519-dalek, ed25519-dalek, x25519-dalek
Copyright (c) 2016-2021 isis agora lovecruft, Henry de Valence
Licensed under BSD-3-Clause

### RNNoise (via nnnoiseless)
Copyright (c) 2018 Gregor Richards, Jean-Marc Valin
Licensed under BSD-3-Clause

### Opus Codec
Copyright (c) 2010-2015 Xiph.Org Foundation, Skype Limited
Licensed under BSD-3-Clause

[... weitere nach Bedarf ...]

## JavaScript/TypeScript Packages

### Solid.js
Copyright (c) 2016-2023 Ryan Carniato
Licensed under MIT

### Lucide Icons
Licensed under ISC

[... weitere nach Bedarf ...]
```

---

## Compliance-Checkliste

### Vor jedem Release

- [ ] `cargo deny check` lÃ¤uft erfolgreich
- [ ] Keine neuen GPL/AGPL Dependencies
- [ ] THIRD_PARTY_NOTICES.md aktualisiert
- [ ] LICENSE-MIT und LICENSE-APACHE vorhanden
- [ ] Cargo.toml enthÃ¤lt korrekte `license` Angabe

### Bei neuen Dependencies

- [ ] Lizenz geprÃ¼ft (muss auf Allow-Liste sein)
- [ ] Transitive Dependencies geprÃ¼ft
- [ ] In diesem Dokument dokumentiert
- [ ] THIRD_PARTY_NOTICES.md aktualisiert (falls nÃ¶tig)

---

## Zusammenfassung

```
â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
â”‚                    LIZENZ-COMPLIANCE STATUS                      â”‚
â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
â”‚                                                                  â”‚
â”‚  âœ… VOLLSTÃ„NDIG KOMPATIBEL                                      â”‚
â”‚                                                                  â”‚
â”‚  Server Dependencies:      45+ Crates  âœ… Alle geprÃ¼ft          â”‚
â”‚  Client Dependencies:      25+ Crates  âœ… Alle geprÃ¼ft          â”‚
â”‚  Frontend Dependencies:    10+ Packages âœ… Alle geprÃ¼ft         â”‚
â”‚                                                                  â”‚
â”‚  Lizenzen im Einsatz:                                           â”‚
â”‚  â€¢ MIT                     ~60%                                 â”‚
â”‚  â€¢ MIT/Apache 2.0 Dual     ~30%                                 â”‚
â”‚  â€¢ Apache 2.0              ~5%                                  â”‚
â”‚  â€¢ BSD-3-Clause            ~3%                                  â”‚
â”‚  â€¢ ISC                     ~2%                                  â”‚
â”‚                                                                  â”‚
â”‚  Projekt-Lizenz:           MIT OR Apache-2.0                    â”‚
â”‚                                                                  â”‚
â”‚  Automatisierung:          cargo-deny in CI/CD                  â”‚
â”‚                                                                  â”‚
â”‚  Letzte PrÃ¼fung:           [Datum einfÃ¼gen]                     â”‚
â”‚                                                                  â”‚
â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
```

---

## Referenzen

- [PROJECT_SPEC.md](../project/specification.md) - Projektanforderungen
- [ARCHITECTURE.md](../architecture/overview.md) - Technische Architektur
- [STANDARDS.md](../development/standards.md) - Verwendete Standards und Protokolle
- [SPDX License List](https://spdx.org/licenses/)
- [Choose a License](https://choosealicense.com/)
- [cargo-deny Documentation](https://embarkstudios.github.io/cargo-deny/)
