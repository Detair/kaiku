# Client Source Code

<!-- Parent: ../AGENTS.md -->

## Purpose

This directory contains the Solid.js frontend application source code for the VoiceChat platform. It implements a reactive, type-safe UI with real-time WebSocket communication, Tauri-native commands, and a dual-mode architecture (native Tauri app + browser fallback).

**Key Technologies:** Solid.js, TypeScript, UnoCSS, Solid Router, Tauri 2.0 API

## Key Files

- **`index.tsx`** — Application entry point. Sets up Solid.js rendering, router, and imports global styles (UnoCSS, themes, highlight themes).

- **`App.tsx`** — Route definitions and layout wrapper. Exports `AppRoutes` component with protected routes (via `AuthGuard`), layout wrapper (with theme initialization via `onMount`), and route structure (`/login`, `/register`, `/invite/:code`, `/demo`, `/*` for main app).

- **`vite-env.d.ts`** — TypeScript environment declarations for Vite.

## Subdirectories

- **`components/`** ([see AGENTS.md](components/AGENTS.md)) — Reusable UI components organized by domain (auth, call, channels, guilds, home, layout, messages, settings, social, ui, voice).

- **`lib/`** ([see AGENTS.md](lib/AGENTS.md)) — Core utilities and type-safe API wrappers.
  - `tauri.ts` — Dual-mode Tauri command wrappers (native commands + browser HTTP fallback)
  - `types.ts` — TypeScript interfaces for domain models
  - `utils.ts` — Utility functions
  - `webrtc/` — WebRTC implementation for voice calls

- **`pages/`** — Standalone page components (currently just `ThemeDemo.tsx` for theme testing).

- **`stores/`** ([see AGENTS.md](stores/AGENTS.md)) — Solid.js global state stores using `createStore` (auth, call, channels, dms, friends, guilds, messages, presence, theme, voice, websocket).

- **`styles/`** — Global CSS and theme files.
  - `global.css` — Base styles
  - `themes.css` — Theme variable definitions (focused-hybrid, solarized-dark, solarized-light)
  - `highlight-theme.css` — Syntax highlighting for code blocks

- **`views/`** ([see AGENTS.md](views/AGENTS.md)) — Top-level application views (Login, Register, Main, InviteJoin) that compose components and stores.

## For AI Agents

### Solid.js Patterns

**Reactive State:**

```typescript
import { createSignal, createStore } from "solid-js/store";

// Signals for simple values
const [count, setCount] = createSignal(0);

// Stores for complex objects (auto-tracked fine-grained reactivity)
const [state, setState] = createStore({ messages: [], loading: false });
setState("messages", (msgs) => [...msgs, newMsg]);
```

**Lifecycle & Effects:**

```typescript
import { onMount, onCleanup, createEffect } from "solid-js";

onMount(async () => {
  await initTheme();
  // Setup code
  onCleanup(() => {
    // Cleanup code
  });
});

createEffect(() => {
  // Runs when reactive dependencies change
  console.log(count());
});
```

**Control Flow (avoid .map, use <For>):**

```typescript
import { For, Show } from "solid-js";

<Show when={user()} fallback={<Login />}>
  <Main />
</Show>

<For each={messages()}>
  {(msg) => <MessageItem message={msg} />}
</For>
```

### Component Conventions

1. **Type-safe Props:**

   ```typescript
   interface MyComponentProps {
     title: string;
     onClose?: () => void;
   }

   const MyComponent: Component<MyComponentProps> = (props) => { ... };
   ```

2. **ParentProps for Layout Components:**

   ```typescript
   import { ParentProps } from "solid-js";

   const Layout: Component<ParentProps> = (props) => (
     <div>{props.children}</div>
   );
   ```

3. **Avoid Destructuring Props:**

   ```typescript
   // BAD (breaks reactivity)
   const { value } = props;

   // GOOD
   <div>{props.value}</div>
   ```

### Dual-Mode Architecture

The app runs in two modes:

1. **Tauri Native (production):**
   - Commands via `@tauri-apps/api/core` `invoke()`
   - WebSocket via Tauri backend
   - Voice via native Rust WebRTC

2. **Browser Mode (development):**
   - Commands via HTTP fetch to `localhost:8080`
   - WebSocket via native browser WebSocket API
   - Voice not available (requires native app)
   - JWT token management in localStorage with auto-refresh

**Detection:** `lib/tauri.ts` checks `window.__TAURI__` to branch logic.

### Store Organization

All stores follow this pattern:

```typescript
import { createStore } from "solid-js/store";

interface MyState {
  items: Item[];
  loading: boolean;
}

const [state, setState] = createStore<MyState>({ items: [], loading: false });

export async function loadItems() {
  setState("loading", true);
  const items = await tauri.getItems();
  setState({ items, loading: false });
}

export { state as myState };
```

### Theme System

Themes are data-driven and CSS-variable-based:

```typescript
// Available themes
type ThemeName = "focused-hybrid" | "solarized-dark" | "solarized-light";

// Apply via data-theme attribute
document.documentElement.setAttribute("data-theme", theme);
```

CSS variables defined in `styles/themes.css` for each theme (e.g., `--bg-primary`, `--text-primary`, etc.).

### WebSocket Event Handling

WebSocket events are handled via:

1. **Tauri:** Event listeners via `@tauri-apps/api/event` (see `stores/websocket.ts`)
2. **Browser:** Native WebSocket message handlers

Pattern: Stores subscribe to events and update state reactively.

### Routing

Uses `@solidjs/router`:

```typescript
import { Route } from "@solidjs/router";

// Protected routes wrap components in AuthGuard
<Route path="/dashboard" component={ProtectedDashboard} />
```

Navigation: `import { useNavigate } from "@solidjs/router";`

### File Uploads

Uses `FormData` with fetch (both Tauri and browser support):

```typescript
const formData = new FormData();
formData.append("file", file);
formData.append("message_id", messageId);

await fetch(`${baseUrl}/api/messages/upload`, {
  method: "POST",
  headers: { Authorization: `Bearer ${token}` },
  body: formData,
});
```

### Error Handling

HTTP errors throw with structured messages:

```typescript
if (!response.ok) {
  const error = await response
    .json()
    .catch(() => ({ message: response.statusText }));
  throw new Error(error.message || error.error || "Request failed");
}
```

Catch errors in components and display via UI state or notifications.

### Style Conventions

- **UnoCSS:** Tailwind-like utility classes (`class="flex items-center gap-2"`)
- **Theme Variables:** Use CSS variables for colors (`class="bg-background-primary text-text-primary"`)
- **Icons:** lucide-solid for icon components

### Common Patterns

**API Call with Loading State:**

```typescript
const [loading, setLoading] = createSignal(false);

async function handleSubmit() {
  setLoading(true);
  try {
    await tauri.someCommand(data);
  } catch (err) {
    console.error(err);
  } finally {
    setLoading(false);
  }
}
```

**Reactive Derived State:**

```typescript
const isAdmin = () => user()?.role === "admin";

<Show when={isAdmin()}>
  <AdminPanel />
</Show>
```

**Token Refresh:**
Browser mode automatically schedules token refresh 60 seconds before expiration. Manual refresh available via `refreshAccessToken()` in `lib/tauri.ts`.

### Testing

- Unit tests: Component logic (signals, stores)
- Integration tests: API wrappers with mocked responses
- E2E tests: Playwright (runs against built Tauri app)

### Performance

- Solid.js compiles to fine-grained reactivity (no VDOM diffing)
- Store mutations are granular (only affected parts re-render)
- Use `<For>` instead of `.map()` for keyed lists
- Avoid creating new objects/functions in render (breaks memoization)

**Example (avoid):**

```typescript
<Button onClick={() => handleClick(id)} />  // Creates new function every render
```

**Better:**

```typescript
const handleClick = () => handle(id);
<Button onClick={handleClick} />
```

Or use inline handlers only for rare UI events (not per-item in lists).
