# Tauri Rust Backend

<!-- Parent: ../AGENTS.md -->

## Purpose

Tauri 2.0 Rust backend for the Kaiku desktop client. Provides native functionality for audio I/O, WebRTC peer connections, cryptography, and IPC with the Solid.js frontend.

**Performance targets:**
- Idle RAM: <80MB
- Idle CPU: <1%
- Voice latency: <50ms end-to-end
- Startup: <3s

**Architecture:**
- **IPC:** Tauri commands (async Rust functions exposed to WebView)
- **State management:** AppState with Arc<RwLock<T>> for shared mutable state
- **Audio:** cpal (cross-platform I/O) + opus (codec)
- **WebRTC:** webrtc-rs for peer connections, DTLS-SRTP for voice
- **Crypto:** vodozemac (E2EE text), keyring (secure token storage)

## Key Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Crate configuration, dependencies (webrtc, cpal, opus, vodozemac, tauri) |
| `tauri.conf.json` | Tauri app config (window size, bundle settings, build commands) |
| `build.rs` | Tauri build script (generates command bindings) |
| `src/lib.rs` | Library entry point, AppState definition, command registration |
| `src/main.rs` | Binary entry point (calls vc_client::run()) |

## Subdirectories

### `src/`
Core application modules:

- **`audio/`** — Audio capture/playback via cpal, opus encoding/decoding, device enumeration
- **`commands/`** — Tauri command handlers (auth, chat, voice, settings, websocket)
- **`crypto/`** — E2EE text chat (vodozemac), keyring integration for token storage
- **`network/`** — WebSocket manager for real-time server communication
- **`webrtc/`** — WebRTC peer connections, SDP handling, ICE candidate exchange

### Other Directories

- **`capabilities/`** — Tauri capabilities (security permissions) configuration
- **`gen/`** — Tauri-generated code (command bindings, IPC types)
- **`icons/`** — Application icons for different platforms

## For AI Agents

### Tauri Command Conventions

All commands in `src/commands/` follow this pattern:

```rust
use tauri::State;

#[tauri::command]
pub async fn example_command(
    arg: String,
    state: State<'_, AppState>,
) -> Result<ResponseType, String> {
    // Access shared state
    let auth = state.auth.read().await;

    // Return Result<T, String> for automatic error serialization
    Ok(response_data)
}
```

**Rules:**
- All commands are `async` (Tauri runs them on tokio runtime)
- Use `State<'_, AppState>` to access shared application state
- Return `Result<T, String>` where T is serializable
- Error strings are sent to frontend as-is (no stack traces in production)
- Register in `lib.rs` via `tauri::generate_handler![]`

### AppState Structure

Located in `src/lib.rs`:

```rust
pub struct AppState {
    pub http: HttpClient,                                // reqwest client
    pub auth: Arc<RwLock<AuthState>>,                   // tokens, user
    pub websocket: Arc<RwLock<Option<WebSocketManager>>>, // WS connection
    pub voice: Arc<RwLock<Option<VoiceState>>>,         // WebRTC + audio
}
```

**Access patterns:**
- Read-only: `state.auth.read().await`
- Write: `state.auth.write().await`
- Voice initialization: `state.ensure_voice().await` (lazy-init pattern)

### Frontend IPC

Frontend calls commands via `@tauri-apps/api/core`:

```typescript
import { invoke } from '@tauri-apps/api/core';

// TypeScript signature matches Rust command
const user = await invoke<User>('get_current_user');
const channels = await invoke<Channel[]>('get_channels', { serverId: '...' });
```

**Event emission** (Rust → Frontend):
```rust
use tauri::Manager;

app.emit("voice-state-changed", payload)?;
```

### Performance Notes

**Hot paths** (must be zero-copy where possible):
- Audio capture → opus encode → WebRTC send
- WebRTC receive → opus decode → audio playback
- WebSocket message handling

**Avoid in audio pipeline:**
- Allocations (use pre-allocated buffers)
- Blocking operations (use tokio channels)
- Lock contention (voice state is write-locked only during setup/teardown)

**Memory management:**
- AppState lives for entire app lifetime (singleton)
- VoiceState is lazy-initialized (only when joining voice)
- WebSocketManager reconnects on network errors (stateful)

### Testing Commands

Test commands in isolation via `#[cfg(test)]` modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_login() {
        let state = AppState::new();
        // Test logic
    }
}
```

For integration tests requiring Tauri context, see `../tests/` (if exists).

### Dependencies

**License-critical** (must check before adding):
```bash
cargo deny check licenses
```

**Allowed:** MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0
**Forbidden:** GPL, AGPL, LGPL (static linking violates our MIT/Apache dual license)

**Current key deps:**
- `tauri = "2"` (MIT/Apache-2.0)
- `webrtc = "0.9"` (MIT/Apache-2.0)
- `cpal = "0.15"` (Apache-2.0)
- `opus = "0.3"` (BSD-3-Clause wrapper, links to libopus)
- `vodozemac = "0.5"` (Apache-2.0, Matrix E2EE library)
- `keyring = "2"` (MIT/Apache-2.0, OS credential storage)

### Build System

**Development:**
```bash
cargo tauri dev  # Runs frontend dev server + Rust in debug mode
```

**Production:**
```bash
cargo tauri build  # Bundles frontend + Rust into platform-specific installer
```

**Platform outputs:**
- Linux: AppImage (self-contained, includes WebView)
- macOS: .app bundle + .dmg (requires macOS 10.15+)
- Windows: .exe installer (embeds WebView2 bootstrapper)

### Module Boundaries

**Commands → Core modules:**
- Commands are thin adapters (validation, state access)
- Business logic lives in `audio/`, `webrtc/`, `crypto/`, `network/`
- Commands should not contain >20 lines of logic

**Example structure:**
```
commands/voice.rs       → validates args, locks state
  ↓
webrtc/client.rs        → SDP offer/answer, ICE handling
  ↓
audio/capture.rs        → cpal streams, opus encoding
```

### Security Notes

**Token storage:**
- Access tokens stored in memory (AppState.auth)
- Refresh tokens stored in OS keyring (keyring crate)
- Never log tokens (even in debug mode)

**WebRTC security:**
- DTLS-SRTP for voice encryption (server-trusted, not E2EE)
- Future: MLS for "Paranoid Mode" E2EE voice

**Input validation:**
- All command arguments are user-controlled
- Validate before passing to internal modules
- Use `thiserror` for typed errors, `anyhow` for command handlers

### Observability

**Logging:**
```rust
use tracing::{info, warn, error, debug};

#[tracing::instrument(skip(state))]  // Auto-log function entry/exit
async fn my_command(arg: String, state: State<'_, AppState>) -> Result<(), String> {
    info!("Processing command with arg: {}", arg);
    // ...
}
```

**Log levels:**
- `error!`: User-visible failures (auth errors, network failures)
- `warn!`: Recoverable issues (reconnection attempts, degraded mode)
- `info!`: State changes (connected, joined voice, logged out)
- `debug!`: Detailed flow (SDP exchange, audio buffer states)
- `trace!`: Hot-path tracing (disabled in release)

**Filters:**
```bash
RUST_LOG=vc_client=debug cargo tauri dev  # Debug this crate only
RUST_LOG=trace cargo tauri dev            # All dependencies (noisy)
```

### Common Patterns

**Lazy state initialization:**
```rust
// VoiceState is expensive, only init when needed
state.ensure_voice().await?;
let mut voice = state.voice.write().await;
let voice = voice.as_mut().ok_or("Voice not initialized")?;
```

**WebSocket event dispatch:**
```rust
// network/websocket.rs spawns task that emits events
app.emit("ws:message", message)?;
app.emit("ws:error", error)?;
```

**Audio pipeline:**
```
cpal input → opus encode → mpsc channel → WebRTC send
WebRTC recv → mpsc channel → opus decode → cpal output
```

Channels are bounded (drop old frames on overflow to prevent latency buildup).

### References

- Tauri 2.0 docs: https://v2.tauri.app
- webrtc-rs examples: https://github.com/webrtc-rs/webrtc/tree/master/examples
- cpal examples: https://github.com/RustAudio/cpal/tree/master/examples
- vodozemac docs: https://docs.rs/vodozemac
