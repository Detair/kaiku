# License Compliance

This document tracks third-party dependencies and their licenses for the VoiceChat project.

## Fonts

### Press Start 2P
- **License:** SIL Open Font License 1.1 (OFL-1.1)
- **Source:** https://fonts.google.com/specimen/Press+Start+2P
- **Author:** CodeMan38
- **Usage:** Bundled font for pixel art theme UI elements
- **Compliance:** OFL-1.1 permits bundling and redistribution with attribution. Font name may not be used for derived works without permission.

## Email

### lettre
- **License:** MIT
- **Source:** https://crates.io/crates/lettre
- **Version:** 0.11
- **Usage:** SMTP email transport for transactional emails (password reset)
- **Compliance:** MIT licensed, fully compatible with project license

## Checking Dependency Licenses

### Rust Dependencies
```bash
cargo deny check licenses
```

### JavaScript Dependencies
```bash
# Check for problematic licenses
bun pm licenses
```

## Allowed Licenses
- MIT
- Apache-2.0
- BSD-2-Clause
- BSD-3-Clause
- ISC
- Zlib
- CC0-1.0
- Unlicense
- MPL-2.0
- Unicode-DFS-2016
- OFL-1.1 (for fonts)

## Prohibited Licenses
- GPL-2.0
- GPL-3.0
- AGPL-3.0
- LGPL-2.0, LGPL-2.1, LGPL-3.0 (for static linking)
- SSPL
- Proprietary
