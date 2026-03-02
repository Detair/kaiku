# <span style="color: #88C0D0;">Kaiku Architecture Overview</span>

> [!WARNING]  
> **Early Development Note:** Kaiku is heavily in development. The architecture described below outlines the target technical implementation that is actively being built.

Kaiku is engineered for one specific purpose: to provide the lowest latency, most secure voice communication possible, while using so little system resources that you forget it's running. 

To achieve this, we combined three specific core technologies: **SolidJS**, **Tauri (Rust)**, and **WebRTC**.

---

## <span style="color: #88C0D0;">1. The Frontend: SolidJS</span>
The UI is built using SolidJS. 

**Why not React or Vue?** 
SolidJS entirely drops the "Virtual DOM" (VDOM) overhead. When data changes, Solid updates the Exact DOM node directly. This results in:
- Unmatched reactivity and speed.
- Significantly lower memory overhead.
- Smoother 60FPS UI animations for our premium glassmorphic design, even while a heavy game is running in the background.

## <span style="color: #88C0D0;">2. The Backend Engine: Tauri (Rust)</span>
Instead of shipping an entire Chromium browser instance disguised as a chat app (like Electron apps do), Kaiku relies on Tauri.

**Why Tauri?**
- **Native OS Integration**: Tauri uses the operating system's native webview (e.g., WebKitGTK on Linux) to render the SolidJS frontend, dropping the bundle size from hundreds of megabytes down to just a few.
- **Rust Backend**: All heavy lifting (system presence detection, global push-to-talk hooks, and overlay rendering) is written in Rust. This guarantees memory safety, zero-cost abstractions, and blazing-fast execution speeds without stuttering.
- **Minimal Footprint**: Because the heavy V8 engine is stripped away, Kaiku sips CPU cycles. Your frame pacing stays perfect.

## <span style="color: #88C0D0;">3. The Audio Core: WebRTC</span>
Kaiku utilizes deep, customized WebRTC implementations for all real-time media.

- **SFU Architecture**: All voice calls — including 1:1 DM calls — are routed through a Selective Forwarding Unit (SFU). This simplifies the networking model, provides consistent latency behavior, and makes it straightforward to add participants or record diagnostics without renegotiating peer connections.
- **End-to-End Encryption**: All voice streams are encrypted with DTLS-SRTP as the baseline, with a planned "Paranoid Mode" upgrade to MLS for true end-to-end encryption.

---
*Detailed component diagrams and IPC (Inter-Process Communication) documentation will be added as the codebase matures.*
