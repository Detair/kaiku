# VoiceChat

A self-hosted voice and text chat platform for gaming communities.

[![CI](https://github.com/yourorg/voicechat/actions/workflows/ci.yml/badge.svg)](https://github.com/yourorg/voicechat/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)

## Features

- **Low Latency Voice Chat** – WebRTC-based with Opus codec, optimized for gaming
- **End-to-End Encryption** – Text messages encrypted with Olm/Megolm
- **Self-Hosted** – Your data stays on your server
- **Lightweight Client** – Tauri-based desktop app with minimal resource usage
- **SSO Support** – Integrate with Authentik, Keycloak, Azure AD, and more
- **Open Source** – MIT/Apache-2.0 dual licensed

## Quick Start

### Server (Docker)

```bash
# Clone the repository
git clone https://github.com/yourorg/voicechat.git
cd voicechat

# Copy and edit environment file
cp .env.example .env
# Edit .env with your settings

# Start the server
cd infra/compose
docker compose up -d
```

### Desktop Client

Download the latest release from [Releases](https://github.com/yourorg/voicechat/releases).

## Development

### Prerequisites

- Rust 1.82+ (`rustup update stable`)
- Bun 1.1+ (for package management and scripts)
- Node.js 18+ (required for Playwright tests)
- Docker & Docker Compose

### Quick Setup

```bash
# Run the setup script (installs deps, starts Docker, runs migrations)
./scripts/dev-setup.sh

# Start the server in watch mode
make dev

# In another terminal, start the client
make client
```

### Manual Setup

```bash
# Start development services (PostgreSQL, Redis, MinIO, MailHog)
make docker-up

# Run database migrations
make db-migrate

# Install client dependencies
cd client && bun install

# Run server
cargo run -p vc-server

# Run client (in another terminal)
cd client && bun run tauri dev
```

### Useful Commands

```bash
make help         # Show all available commands
make dev          # Start server in watch mode (auto-reload)
make client       # Start client in dev mode
make test         # Run all tests
make check        # Run cargo check + clippy
make db-reset     # Reset database
make docker-logs  # View Docker service logs
```

### Test Users

After setup, create test users:

```bash
./scripts/create-test-users.sh
```

Default credentials: `admin/admin123`, `alice/password123`, `bob/password123`

## Project Structure

```
voicechat/
├── server/          # Backend server (Rust/Axum)
├── client/          # Desktop client (Tauri + Solid.js)
├── shared/          # Shared Rust libraries
│   ├── vc-common/   # Common types and protocols
│   └── vc-crypto/   # E2EE cryptography
├── infra/           # Infrastructure (Docker, scripts)
├── docs/            # Documentation
└── specs/           # Project specifications
```

## Documentation

- [Quick Start Guide](docs/setup/quick-start.md)
- [Configuration](docs/setup/configuration.md)
- [Architecture](specs/ARCHITECTURE.md)
- [API Reference](docs/api/)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
