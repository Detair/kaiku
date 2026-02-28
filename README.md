<div align="center">
  <img src="kaiku-landing/assets/images/extracted/floki_logo_circle.png" alt="Kaiku Logo" width="250" />
  <h1 style="color: #88C0D0;">Kaiku</h1>
  <p><b style="color: #B48EAD;">The Echo of Victory — Next-Gen Voice Chat for Gamers</b></p>
  <p>
    <a href="https://github.com/detair/canis/releases"><img alt="Platform" src="https://img.shields.io/badge/platform-linux-2E3440.svg?style=flat-square&logo=linux&logoColor=88C0D0" /></a>
    <a href="https://github.com/detair/canis/issues"><img alt="Issues" src="https://img.shields.io/github/issues/detair/canis?style=flat-square&color=88C0D0&labelColor=2E3440" /></a>
    <a href="https://github.com/detair/canis/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-B48EAD.svg?style=flat-square&labelColor=2E3440" /></a>
  </p>
</div>

---

> [!WARNING]
> **Early Development Stage!** Kaiku is currently an active work-in-progress and is **not yet ready for production usage**. Architecture, codebase, and features are subject to significant changes. 

## <span style="color: #88C0D0;">🐺 What is Kaiku?</span>

**Kaiku** (Finnish for *Echo*) is a modern, high-performance voice chat and Server-Centric overlay built specifically for gamers and esports teams who demand low-latency, reliable communication without system bloat. 

Echoing the quiet precision of the North, Kaiku ensures your pack stays connected, pulling inspiration from the sleek aesthetics of CachyOS Nordic combined with a robust WebRTC and Tauri-powered architecture.

## <span style="color: #88C0D0;">✨ Key Capabilities</span>

- ⚡ <b style="color: #B48EAD;">Low Latency Audio</b>: Optimized routing for immediate delivery of your callouts.
- 🛡️ <b style="color: #B48EAD;">Absolute Data Freedom</b>: Stop paying with your data. Self-hosted and End-to-End Encrypted so your team's strategy and conversations stay strictly yours.
- 💻 <b style="color: #B48EAD;">Minimal Resource Footprint</b>: Uses minimal CPU. Play your games without FPS drops or frame pacing issues.

## <span style="color: #88C0D0;">🖥️ Deploying Your Pack (Self-Hosting)</span>

Kaiku is designed from the ground up for **self-hosting**, giving you absolute sovereignty over your community's data and communication infrastructure. Unlike corporate alternatives that harvest your conversations, Kaiku ensures true freedom. Whether you are running a casual guild server or a massive esports organization, Kaiku scales with you.

👉 **[Read the Official Admin Guide](docs/admin-guide/self-hosting.md)** to learn how to deploy Kaiku using Docker, configure your TURN/STUN servers, and manage user access.

## <span style="color: #88C0D0;">🏗️ Architecture</span>

Built for maximum performance and minimum bloat:
- **Frontend**: Solid.js for extreme reactivity and a premium glassmorphic UI.
- **Backend/Desktop Integration**: Tauri (Rust) for native OS-level performance with a tiny memory footprint.
- **Audio Engine**: Fine-tuned WebRTC for flawless real-time communication.

👉 **[Explore the Architecture Overview](docs/developer-guide/architecture/architecture.md)**

## <span style="color: #88C0D0;">🔒 Security & Privacy</span>

We don't want your data. All direct messages and private group calls utilize **Olm cryptographic ratchet** (similar to Signal) for true end-to-end encryption. 

👉 **[Review our Security & Encryption Protocols](docs/developer-guide/security/security.md)**

## <span style="color: #88C0D0;">🚀 Quick Start (Development)</span>

1. Clone the repository: `git clone https://github.com/detair/canis.git`
2. Install dependencies: `bun install`
3. Run the development environment: `bun run tauri dev`

## <span style="color: #88C0D0;">🤝 Contributing & License</span>

We welcome contributions! Please read our `CONTRIBUTING.md` (coming soon). Kaiku is released under the **MIT License**.

---
<div align="center">
  <p><i style="color: #D8DEE9;">"Don't let your voice get lost in the noise. Join the Pack."</i></p>
  <p><small><b>Disclaimer:</b> This project, including its code and documentation, was created with the assistance of Artificial Intelligence.</small></p>
</div>
