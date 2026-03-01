# License Compliance

This document tracks all direct third-party dependencies and their licenses for the Kaiku project.

**Project License:** MIT OR Apache-2.0 (Dual License)

**Last Updated:** 2026-01-31

---

## Rust Dependencies

### Async Runtime & Networking

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| tokio | 1.49 | MIT | Async runtime |
| futures | 0.3 | MIT OR Apache-2.0 | Async utilities and combinators |
| tokio-util | 0.7.18 | MIT | Async I/O helpers (server only) |
| tokio-tungstenite | 0.28 | MIT | WebSocket client/server |

### Web Framework & HTTP

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| axum | 0.8 | MIT | HTTP/WebSocket server framework |
| tower | 0.5 | MIT | Middleware framework |
| tower-http | 0.6 | MIT | HTTP middleware (CORS, compression, tracing) |
| reqwest | 0.13 | MIT OR Apache-2.0 | HTTP client (client + server dev-deps) |

### WebRTC & Voice

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| webrtc | 0.11 | MIT OR Apache-2.0 | WebRTC stack (SFU, DTLS-SRTP) |
| cpal | 0.17 | Apache-2.0 | Cross-platform audio I/O (client) |
| opus | 0.3 | MIT/Apache-2.0 | Opus audio codec bindings (client) |
| rodio | 0.21 | MIT OR Apache-2.0 | Audio playback (client) |

### Database & Storage

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| sqlx | 0.8.6 | MIT OR Apache-2.0 | PostgreSQL async driver with compile-time query checking |
| fred | 10.1 | MIT | Valkey/Redis async client |
| rusqlite | 0.32 | MIT | SQLite for local client storage (bundled) |
| aws-sdk-s3 | 1.x | Apache-2.0 | S3-compatible object storage |
| aws-config | 1.x | Apache-2.0 | AWS SDK configuration |

### Authentication & Authorization

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| jsonwebtoken | 9.3 | MIT | JWT token creation and validation |
| argon2 | 0.5 | MIT OR Apache-2.0 | Argon2id password hashing |
| totp-rs | 5.7 | MIT | TOTP-based MFA with QR code generation |
| openidconnect | 3.5 | MIT | OpenID Connect / OAuth 2.0 client |

### Cryptography & E2EE

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| vodozemac | 0.9 | Apache-2.0 | Olm/Megolm E2EE (Matrix protocol) |
| rustls | 0.23 | Apache-2.0 OR ISC OR MIT | TLS 1.3 implementation |
| aes-gcm | 0.10 | Apache-2.0 OR MIT | AES-GCM symmetric encryption |
| hkdf | 0.12 | MIT OR Apache-2.0 | HMAC-based key derivation |
| sha2 | 0.10 | MIT OR Apache-2.0 | SHA-256/SHA-512 hashing |
| zeroize | 1.8 | Apache-2.0 OR MIT | Secure memory zeroing for secrets |
| bs58 | 0.5 | MIT/Apache-2.0 | Base58 encoding for recovery keys (vc-crypto) |
| getrandom | 0.2 | MIT OR Apache-2.0 | OS-level random number generation (vc-crypto) |

### Serialization & Data

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| serde | 1.x | MIT OR Apache-2.0 | Serialization/deserialization framework |
| serde_json | 1.x | MIT OR Apache-2.0 | JSON serialization |
| uuid | 1.x | Apache-2.0 OR MIT | UUIDv7 generation |
| chrono | 0.4 | MIT OR Apache-2.0 | Date/time handling |
| bytes | 1.x | MIT | Byte buffer utilities |
| hex | 0.4 | MIT OR Apache-2.0 | Hex encoding/decoding |
| base64 | 0.22 | MIT OR Apache-2.0 | Base64 encoding/decoding |
| bitflags | 2.4 | MIT OR Apache-2.0 | Bitflag types for permissions |
| url | 2.5 | MIT OR Apache-2.0 | URL parsing (client) |

### Utilities

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| thiserror | 2.x | MIT OR Apache-2.0 | Derive macro for error types |
| anyhow | 1.x | MIT OR Apache-2.0 | Application-level error handling |
| rand | 0.8 | MIT OR Apache-2.0 | Random number generation |
| lazy_static | 1.4 | MIT OR Apache-2.0 | Lazily initialized statics |
| regex | 1.x | MIT OR Apache-2.0 | Regular expressions |
| dashmap | 6.1 | MIT | Lock-free concurrent hash map |
| validator | 0.20 | MIT | Input validation with derive macros |
| dotenvy | 0.15 | MIT | Environment variable loading (server) |

### API Documentation

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| utoipa | 5.4 | MIT OR Apache-2.0 | OpenAPI spec generation |
| utoipa-swagger-ui | 9.0 | MIT OR Apache-2.0 | Swagger UI serving |

### Content Processing

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| pulldown-cmark | 0.13 | MIT | Markdown parsing |
| mime_guess | 2.0 | MIT | MIME type detection for file uploads |
| image | 0.25 | MIT OR Apache-2.0 | Image encoding (PNG thumbnails) |

### Email

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| lettre | 0.11 | MIT | SMTP email transport (password reset, verification) |

### Screen Capture & Video Encoding (Client)

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| scap | 0.0.8 | MIT | Cross-platform screen capture |
| vpx-encode | 0.3 | MIT | VP9 video encoding (wraps libvpx, BSD-3-Clause) |
| openh264 | 0.6 | BSD-2-Clause | H.264 fallback encoding (Cisco patent-free binary) |

### Desktop Client (Tauri)

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| tauri | 2.x | Apache-2.0 OR MIT | Desktop application framework |
| tauri-build | 2.x | Apache-2.0 OR MIT | Build tooling for Tauri |
| tauri-plugin-shell | 2.x | Apache-2.0 OR MIT | Shell command plugin |
| keyring | 2.3 | MIT OR Apache-2.0 | OS keychain for secure credential storage |
| sysinfo | 0.34 | MIT | System/process information for capture sources |
| arboard | 3.6 | MIT OR Apache-2.0 | Clipboard access |

### Logging & Observability

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| tracing | 0.1 | MIT | Structured logging/tracing framework |
| tracing-subscriber | 0.3 | MIT | Log output formatting and filtering |

### Dev Dependencies (not shipped in production)

| Crate | Version | License | Usage |
|-------|---------|---------|-------|
| tokio-test | 0.4 | MIT | Async test utilities |
| serial_test | 3.2 | MIT | Sequential test execution |
| tempfile | 3.x | MIT OR Apache-2.0 | Temporary files for tests |

---

## JavaScript / TypeScript Dependencies

### Runtime Dependencies (shipped to users)

| Package | Version | License | Usage |
|---------|---------|---------|-------|
| solid-js | ^1.9.10 | MIT | Reactive UI framework |
| @solidjs/router | ^0.10.0 | MIT | Client-side routing |
| @tauri-apps/api | ^2.0.0 | Apache-2.0 OR MIT | Tauri IPC bridge |
| @tauri-apps/plugin-shell | ^2.0.0 | MIT OR Apache-2.0 | Shell command invocation |
| @floating-ui/dom | ^1.7.5 | MIT | Tooltip/popover positioning |
| dompurify | ^3.3.1 | MPL-2.0 OR Apache-2.0 | HTML sanitization (XSS prevention) |
| highlight.js | ^11.11.1 | BSD-3-Clause | Syntax highlighting for code blocks |
| marked | ^17.0.1 | MIT | Markdown rendering |
| mermaid | ^11.12.2 | MIT | Diagram rendering in messages |

### Dev Dependencies (not shipped in production)

| Package | Version | License | Usage |
|---------|---------|---------|-------|
| typescript | ^5.3.0 | Apache-2.0 | TypeScript compiler |
| vite | ^5.0.0 | MIT | Build tool and dev server |
| vite-plugin-solid | ^2.8.0 | MIT | Solid.js Vite integration |
| vitest | ^4.0.18 | MIT | Unit test framework |
| @playwright/test | ^1.57.0 | Apache-2.0 | E2E test framework |
| @solidjs/testing-library | ^0.8.10 | MIT | Component test utilities |
| jsdom | ^27.4.0 | MIT | DOM emulation for tests |
| eslint | ^8.0.0 | MIT | Linting |
| @typescript-eslint/eslint-plugin | ^6.0.0 | MIT | TypeScript lint rules |
| @typescript-eslint/parser | ^6.0.0 | BSD-2-Clause | TypeScript ESLint parser |
| eslint-plugin-solid | ^0.13.0 | MIT | Solid.js lint rules |
| prettier | ^3.0.0 | MIT | Code formatting |
| unocss | ^0.58.0 | MIT | Atomic CSS engine |
| @unocss/preset-uno | ^0.58.0 | MIT | Default UnoCSS preset |
| @unocss/preset-icons | ^0.58.0 | MIT | Icon preset for UnoCSS |
| @unocss/reset | ^0.58.0 | MIT | CSS reset |
| lucide-solid | ^0.300.0 | ISC | Icon library |
| @rollup/plugin-commonjs | ^29.0.0 | MIT | CommonJS module support |
| @types/dompurify | ^3.2.0 | MIT | DOMPurify type definitions |
| @types/node | ^20.0.0 | MIT | Node.js type definitions |
| @tauri-apps/cli | ^2.0.0 | Apache-2.0 OR MIT | Tauri CLI tooling |

---

## Fonts & Assets

### Press Start 2P

- **License:** SIL Open Font License 1.1 (OFL-1.1)
- **Source:** https://fonts.google.com/specimen/Press+Start+2P
- **Author:** CodeMan38 (Cody Boisclair)
- **Usage:** Bundled font for pixel art theme UI elements
- **Compliance:** OFL-1.1 permits bundling and redistribution with attribution. Font name may not be used for derived works without permission.

### VT323

- **License:** SIL Open Font License 1.1 (OFL-1.1)
- **Source:** https://fonts.google.com/specimen/VT323
- **Author:** Peter Hull
- **Usage:** Primary UI font for pixel art theme family
- **Compliance:** OFL-1.1 permits bundling and redistribution with attribution.

---

## Notable License Details

### ring (transitive dependency via rustls/webrtc)

- **License:** MIT AND ISC AND OpenSSL
- **Note:** The OpenSSL license has an advertising clause. This is handled via `deny.toml` clarification and is compatible with the project license.

### dompurify

- **License:** MPL-2.0 OR Apache-2.0
- **Note:** We use it under Apache-2.0 (the OR clause allows choosing either license). Fully compatible.

### vpx-encode / libvpx

- **License:** MIT (vpx-encode crate), BSD-3-Clause (libvpx system library)
- **Note:** Both licenses are compatible with the project license.

### openh264

- **License:** BSD-2-Clause (Rust bindings), Cisco Binary License (pre-built codec)
- **Note:** Cisco provides the OpenH264 binary under a patent-free license. The Rust crate auto-downloads the Cisco binary at build time.

---

## Checking Dependency Licenses

### Rust Dependencies

```bash
# Automated license enforcement (requires cargo-deny)
cargo install cargo-deny
cargo deny check licenses

# List all dependency licenses
cargo metadata --format-version 1 | python3 -c "
import json, sys
data = json.load(sys.stdin)
for pkg in sorted(data['packages'], key=lambda p: p['name']):
    print(f\"{pkg['name']} {pkg['version']} {pkg.get('license', 'UNKNOWN')}\")"
```

### JavaScript Dependencies

```bash
# Check for problematic licenses
bun pm licenses

# Or via node
node -e "
const fs = require('fs');
const pkg = JSON.parse(fs.readFileSync('client/package.json', 'utf8'));
const allDeps = {...pkg.dependencies, ...pkg.devDependencies};
for (const [name] of Object.entries(allDeps).sort()) {
  const dp = JSON.parse(fs.readFileSync('client/node_modules/' + name + '/package.json', 'utf8'));
  console.log(name, dp.version, dp.license);
}"
```

---

## Allowed Licenses

- MIT
- Apache-2.0
- Apache-2.0 WITH LLVM-exception
- BSD-2-Clause
- BSD-3-Clause
- BSL-1.0
- ISC
- Zlib
- CC0-1.0
- Unlicense
- MPL-2.0
- Unicode-DFS-2016
- OFL-1.1 (fonts only)
- OpenSSL (transitive, via ring)

## Prohibited Licenses

- GPL-2.0, GPL-2.0-only, GPL-2.0-or-later
- GPL-3.0, GPL-3.0-only, GPL-3.0-or-later
- AGPL-3.0, AGPL-3.0-only, AGPL-3.0-or-later
- LGPL-2.0, LGPL-2.1, LGPL-3.0 (for static linking)
- SSPL
- Proprietary

## Enforcement

License compliance is enforced via:

- **`deny.toml`** — Automated `cargo deny` checks for Rust dependencies
- **`libsignal-protocol`** — Explicitly banned (AGPL-3.0, incompatible)
- **CI/Manual** — Run `cargo deny check licenses` before adding new dependencies
