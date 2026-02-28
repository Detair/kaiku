<!-- Parent: ../AGENTS.md -->
# Client

## Purpose
Desktop client for the Kaiku platform. Built with Tauri 2.0 (Rust backend) and Solid.js (frontend), providing a lightweight alternative to Electron-based clients.

**Target:** <100MB RAM (vs Discord ~400MB)

## Key Files
- `package.json` - Frontend dependencies (Bun)
- `vite.config.ts` - Vite build configuration
- `uno.config.ts` - UnoCSS styling configuration
- `tsconfig.json` - TypeScript configuration
- `index.html` - Application entry point
- `playwright.config.ts` - E2E test configuration

## Subdirectories
- `src/` - Frontend source (Solid.js/TypeScript) - see src/AGENTS.md
- `src-tauri/` - Desktop backend (Rust/Tauri) - see src-tauri/AGENTS.md
- `public/` - Static assets
- `e2e/` - End-to-end tests (Playwright)
- `dist/` - Build output (gitignored)
- `node_modules/` - Dependencies (gitignored)

## For AI Agents

### Two-Layer Architecture
1. **Frontend (WebView)** - `src/` - Solid.js UI components
2. **Desktop Core (Tauri)** - `src-tauri/` - Rust for audio, crypto, WebRTC

### Running the Client
```bash
# Install dependencies
cd client && bun install

# Development mode
bun run tauri dev
# Or via Makefile
make client

# Build for production
bun run tauri build

# Run tests
bun test
```

### Frontend Guidelines
- Use Solid.js signals for reactive state (`createSignal`, `createStore`)
- Invoke Tauri commands with type safety via `@tauri-apps/api/core`
- Styling with UnoCSS (utility-first)
- Icons from lucide-solid

### Communication with Server
- HTTP/REST for one-time operations
- WebSocket for real-time updates
- WebRTC for voice (via Tauri Rust backend)

### Performance Constraints
- Idle RAM: <80MB
- Idle CPU: <1%
- Startup time: <3s

## Dependencies
- solid-js (UI framework)
- @tauri-apps/api (Tauri IPC)
- unocss (styling)
- lucide-solid (icons)
- vite (build tool)
