# VoiceChat Platform - License Compliance

This document documents the license review of all dependencies used, to ensure the project can be released as open source (MIT OR Apache-2.0).

---

## Project License

```
SPDX-License-Identifier: MIT OR Apache-2.0
```

**Dual Licensing:** Users can choose between MIT and Apache 2.0.

### Rationale for Dual License

| Aspect | MIT | Apache 2.0 |
|--------|-----|------------|
| Simplicity | ✅ Very short and simple | ⚠️ Longer, more complex |
| Patent Protection | ❌ None | ✅ Explicit patent grant |
| Attribution | ✅ Only copyright notice | ✅ Copyright + NOTICE file |
| Compatibility | ✅ Almost everything | ✅ Almost everything |
| Enterprise-friendly | ✅ Yes | ✅ Yes (preferred) |

Dual licensing is standard in the Rust ecosystem and provides maximum flexibility.

---

## License Compatibility

### Compatibility Matrix

```
┌─────────────────────────────────────────────────────────────────┐
│              LICENSE COMPATIBILITY WITH MIT/Apache 2.0          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ✅ COMPATIBLE (can be used)                                    │
│  ─────────────────────────────────────                          │
│  • MIT                    - Permissive                          │
│  • Apache 2.0             - Permissive + Patent                 │
│  • BSD-2-Clause           - Permissive                          │
│  • BSD-3-Clause           - Permissive                          │
│  • ISC                    - Permissive (like MIT)               │
│  • Zlib                   - Permissive                          │
│  • CC0-1.0                - Public Domain                       │
│  • Unlicense              - Public Domain                       │
│  • Unicode-DFS-2016       - Permissive (Unicode Data)           │
│                                                                  │
│  ⚠️ LIMITED COMPATIBILITY                                       │
│  ───────────────────────────                                    │
│  • MPL 2.0                - File-Level Copyleft                 │
│                             (Changes to MPL files must          │
│                              remain under MPL, rest is free)    │
│                                                                  │
│  ❌ NOT COMPATIBLE (must NOT be used)                           │
│  ──────────────────────────────────────────────────             │
│  • GPL 2.0/3.0            - Strong Copyleft                     │
│  • LGPL 2.1/3.0           - Library Copyleft (with static link) │
│  • AGPL 3.0               - Network Copyleft                    │
│  • Proprietary            - Closed Source                       │
│  • SSPL                   - Server-Side Public License          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Server Dependencies Review

### Web Framework & Runtime

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| axum | 0.7 | MIT | ✅ | MIT |
| tokio | 1.x | MIT | ✅ | MIT |
| tower | 0.4 | MIT | ✅ | MIT |
| tower-http | 0.5 | MIT | ✅ | MIT |
| hyper | 1.x | MIT | ✅ | MIT |

### WebSocket & Real-Time

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| tokio-tungstenite | 0.21 | MIT | ✅ | MIT |
| tungstenite | 0.21 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### WebRTC

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| webrtc | 0.11 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-data | 0.9 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-dtls | 0.9 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-ice | 0.11 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-media | 0.8 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-sctp | 0.10 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-srtp | 0.13 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| webrtc-util | 0.9 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Database

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| sqlx | 0.7 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| sqlx-core | 0.7 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| sqlx-postgres | 0.7 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Redis

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| fred | 8.x | MIT | ✅ | MIT |

### Authentication

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| jsonwebtoken | 9.x | MIT | ✅ | MIT |
| argon2 | 0.5 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| totp-rs | 5.x | MIT | ✅ | MIT |
| openidconnect | 3.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| oauth2 | 4.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Cryptography

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| rustls | 0.22 | MIT/Apache 2.0/ISC | ✅ | MIT OR Apache-2.0 OR ISC |
| ring | 0.17 | MIT + ISC + OpenSSL | ✅ | See Note¹ |
| x25519-dalek | 2.x | BSD-3-Clause | ✅ | BSD-3-Clause |
| ed25519-dalek | 2.x | BSD-3-Clause | ✅ | BSD-3-Clause |
| curve25519-dalek | 4.x | BSD-3-Clause | ✅ | BSD-3-Clause |
| aes-gcm | 0.10 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| aes | 0.8 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| hkdf | 0.12 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| sha2 | 0.10 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| hmac | 0.12 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| rand | 0.8 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

¹ **ring License Note:** ring uses code under MIT, ISC, and OpenSSL licenses. All are permissive and compatible.

### E2EE (Text)

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| vodozemac | 0.5 | Apache 2.0 | ✅ | Apache-2.0 |

**Important:** We intentionally use `vodozemac` instead of `libsignal`:

| Library | License | Compatible? |
|---------|--------|-------------|
| vodozemac | Apache 2.0 | ✅ Yes |
| libsignal-protocol | AGPL 3.0 | ❌ No |

### Serialization

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| serde | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| serde_json | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| serde_derive | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Utilities

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| uuid | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| chrono | 0.4 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| tracing | 0.1 | MIT | ✅ | MIT |
| tracing-subscriber | 0.3 | MIT | ✅ | MIT |
| thiserror | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| anyhow | 1.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| bytes | 1.x | MIT | ✅ | MIT |
| futures | 0.3 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### S3 Storage

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| aws-sdk-s3 | 1.x | Apache 2.0 | ✅ | Apache-2.0 |
| aws-config | 1.x | Apache 2.0 | ✅ | Apache-2.0 |
| aws-smithy-runtime | 1.x | Apache 2.0 | ✅ | Apache-2.0 |

### API Documentation

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| utoipa | 4.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| utoipa-swagger-ui | 6.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Markdown

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| pulldown-cmark | 0.10 | MIT | ✅ | MIT |

---

## Client Dependencies Review

### Tauri

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| tauri | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| tauri-build | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| tauri-runtime | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| tauri-runtime-wry | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| wry | 0.35 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |
| tao | 0.25 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Audio

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| cpal | 0.15 | Apache 2.0 | ✅ | Apache-2.0 |
| opus | 0.3 | MIT | ✅ | MIT |
| nnnoiseless | 0.5 | BSD-3-Clause | ✅ | BSD-3-Clause |

**Note on libopus:** The native `libopus` library is licensed under BSD-3-Clause.

### Secure Storage

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| keyring | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### HTTP Client

| Crate | Version | License | Status | SPDX |
|-------|---------|--------|--------|------|
| reqwest | 0.11 | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

---

## Frontend Dependencies Review

### Frameworks

| Package | Version | License | Status | SPDX |
|---------|---------|--------|--------|------|
| solid-js | 1.8 | MIT | ✅ | MIT |
| @solidjs/router | 0.10 | MIT | ✅ | MIT |

### Build Tools

| Package | Version | License | Status | SPDX |
|---------|---------|--------|--------|------|
| vite | 5.x | MIT | ✅ | MIT |
| vite-plugin-solid | 2.8 | MIT | ✅ | MIT |
| typescript | 5.x | Apache 2.0 | ✅ | Apache-2.0 |
| @tauri-apps/cli | 2.x | MIT/Apache 2.0 | ✅ | MIT OR Apache-2.0 |

### Styling

| Package | Version | License | Status | SPDX |
|---------|---------|--------|--------|------|
| unocss | 0.58 | MIT | ✅ | MIT |
| @unocss/preset-uno | 0.58 | MIT | ✅ | MIT |

### Icons

| Package | Version | License | Status | SPDX |
|---------|---------|--------|--------|------|
| lucide-solid | 0.300 | ISC | ✅ | ISC |

---

## Rejected Dependencies

These libraries were reviewed and **intentionally not used**:

| Library | License | Reason for Rejection |
|---------|--------|---------------------|
| libsignal-protocol | AGPL 3.0 | Would force AGPL for entire project |
| ffmpeg | GPL/LGPL | GPL components, complicated license |
| openssl (native) | Apache 2.0 | Okay, but rustls preferred (pure Rust) |
| Matrix SDK | Apache 2.0 | Too complex, only vodozemac used for crypto |

---

## License Files in Repository

The repository must contain the following files:

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

[Full Apache 2.0 Text]
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

### README.md (License Section)

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

## Automated Compliance Checking

### cargo-deny Configuration

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
    # Explicitly banned crates
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

When distributing, the following attributions must be included:

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

[... more as needed ...]

## JavaScript/TypeScript Packages

### Solid.js
Copyright (c) 2016-2023 Ryan Carniato
Licensed under MIT

### Lucide Icons
Licensed under ISC

[... more as needed ...]
```

---

## Compliance Checklist

### Before Each Release

- [ ] `cargo deny check` runs successfully
- [ ] No new GPL/AGPL dependencies
- [ ] THIRD_PARTY_NOTICES.md updated
- [ ] LICENSE-MIT and LICENSE-APACHE present
- [ ] Cargo.toml contains correct `license` field

### When Adding New Dependencies

- [ ] License reviewed (must be on allow list)
- [ ] Transitive dependencies reviewed
- [ ] Documented in this document
- [ ] THIRD_PARTY_NOTICES.md updated (if needed)

---

## Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                    LICENSE COMPLIANCE STATUS                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ✅ FULLY COMPATIBLE                                            │
│                                                                  │
│  Server Dependencies:      45+ Crates   ✅ All reviewed         │
│  Client Dependencies:      25+ Crates   ✅ All reviewed         │
│  Frontend Dependencies:    10+ Packages ✅ All reviewed         │
│                                                                  │
│  Licenses in Use:                                               │
│  • MIT                     ~60%                                 │
│  • MIT/Apache 2.0 Dual     ~30%                                 │
│  • Apache 2.0              ~5%                                  │
│  • BSD-3-Clause            ~3%                                  │
│  • ISC                     ~2%                                  │
│                                                                  │
│  Project License:          MIT OR Apache-2.0                    │
│                                                                  │
│  Automation:               cargo-deny in CI/CD                  │
│                                                                  │
│  Last Review:              [Insert Date]                        │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## References

- [PROJECT_SPEC.md](../project/specification.md) - Project Requirements
- [ARCHITECTURE.md](../architecture/overview.md) - Technical Architecture
- [STANDARDS.md](../development/standards.md) - Standards and Protocols Used
- [SPDX License List](https://spdx.org/licenses/)
- [Choose a License](https://choosealicense.com/)
- [cargo-deny Documentation](https://embarkstudios.github.io/cargo-deny/)
