# Kaiku Documentation

Welcome to the official documentation for Kaiku. This directory is structured to cater to two distinct personas: users/administrators looking to deploy Kaiku, and developers looking to contribute or understand the codebase.

## Directory Structure

```text
docs/
├── admin-guide/        # Guides for self-hosting, configuration, and operations
│   ├── configuration/  # Details on auth methods, feature flags, rate-limiting, and webhooks
│   ├── ops/            # Deployment guides, incident triage, security hardening, and updates
│   ├── rules-and-limits/ # Default limits, message policies, and user conduct guidelines
│   └── self-hosting.md # Overview of self-hosting Kaiku
│
└── developer-guide/    # Deep technical dives into Kaiku's architecture and codebase
    ├── agents/         # Architecture and implementation details of AI agents
    ├── architecture/   # Core architecture, data models, networking, and system diagrams
    ├── design/         # Brand, UI/UX guidelines, and image generation rules
    └── security/       # Cryptographic protocols (Olm/Megolm) and security implementations
```

## Where to Start?

- **I want to run my own Kaiku server!**
  Start with the [Admin Guide - Self Hosting](admin-guide/self-hosting.md) to understand the requirements and deployment process using Docker.

- **I want to understand how Kaiku works under the hood!**
  Head over to the [Developer Guide - Architecture Overview](developer-guide/architecture/overview.md) for a comprehensive look at the client, server, and networking layers.

- **I want to contribute to the UI/UX or brand assets!**
  Review our [Design Guidelines](developer-guide/design/ux-guidelines.md) and [Image Generation Rules](developer-guide/design/image-generation-guidelines.md) to align with Kaiku's premium Nordic aesthetic.

---
> [!NOTE]
> All diagrams within this documentation rely on [Mermaid](https://mermaid.js.org/). Ensure your markdown viewer supports Mermaid to render architectural and flowchart diagrams correctly.
