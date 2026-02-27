# Client Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

UI component library for the Canis voice/chat client. Built with Solid.js for reactive, performant rendering. Organized by feature domain with shared UI primitives.

## Component Domains

### [auth/](auth/AGENTS.md)

Authentication flow components.

- AuthGuard - Route protection and auth state initialization
- Future: Login, Register, MFA forms

### [call/](call/AGENTS.md)

Direct message call interface.

- CallBanner - Call state indicator with controls
- Handles incoming, outgoing, active, and ended call states

### [channels/](channels/AGENTS.md)

Guild channel browser and management.

- ChannelList - Text and voice channel lists
- CreateChannelModal - Channel creation form
- Voice participant integration

### [guilds/](guilds/AGENTS.md)

Guild (server) settings and management.

- GuildSettingsModal - Tabbed settings interface
- InvitesTab - Invite management (owner-only)
- MembersTab - Member list and moderation
- CreateGuildModal - New guild creation

### [home/](home/AGENTS.md)

Home view (DM-focused interface when no guild selected).

- HomeView - Three-column layout orchestrator
- DMSidebar - DM conversations list
- DMConversation - Active DM chat view
- HomeRightPanel - Context panel (friends, activity)

### [layout/](layout/AGENTS.md)

Application shell and primary layout structure.

- AppShell - Main layout grid (server rail, sidebar, main stage)
- ServerRail - Guild switcher (leftmost bar)
- Sidebar - Context-aware sidebar (channels or DMs)
- UserPanel - Bottom panel with user info and controls
- VoiceIsland - Floating voice controls overlay
- CommandPalette - Quick action overlay (Cmd+K)

### [messages/](messages/AGENTS.md)

Chat message display and composition.

- MessageList - Virtualized message list with auto-scroll
- MessageItem - Individual message renderer
- MessageInput - Message composition input
- TypingIndicator - "User is typing..." display

### [settings/](settings/AGENTS.md)

User preferences and app settings.

- SettingsModal - Tabbed settings interface
- AppearanceSettings - Theme, colors, font size
- Audio/Voice settings (planned)

### [social/](social/AGENTS.md)

Friends list and social features.

- FriendsList - Friends list with tabs (Online, All, Pending, Blocked)
- AddFriend - Friend request sender
- Friend management actions (accept, reject, remove)

### [ui/](ui/AGENTS.md)

Primitive, reusable UI components (design system).

- Avatar - User avatar with fallback and status indicator
- StatusIndicator - Online status dot
- CodeBlock - Syntax-highlighted code blocks
- Future: Button, Input, Modal, Tooltip, etc.

### [voice/](voice/AGENTS.md)

Voice channel UI and controls.

- VoiceControls - Mute, deafen, settings buttons
- VoiceParticipants - User list in voice channel
- AudioDeviceSettings - Device selection
- MicrophoneTest - Mic testing modal

## Architecture Patterns

### Component Types

**Smart Components (Containers):**

- Connected to stores via imports
- Handle business logic and data fetching
- Examples: MessageList, ChannelList, FriendsList

**Dumb Components (Presentational):**

- Pure props → render
- No store dependencies
- Reusable across contexts
- Examples: Avatar, Button, MessageItem

**Layout Components:**

- Structure and positioning
- Minimal logic
- Examples: AppShell, Sidebar, HomeView

**Modal Components:**

- Portal-rendered for z-index control
- ESC and backdrop-click to close
- Examples: SettingsModal, CreateChannelModal

### State Management

**Global Stores (Solid Signals):**

- `@/stores/auth` - Authentication state
- `@/stores/guilds` - Guild/server data
- `@/stores/channels` - Channel data
- `@/stores/messages` - Message cache
- `@/stores/dms` - DM conversations
- `@/stores/friends` - Friends and requests
- `@/stores/voice` - Voice channel state
- `@/stores/call` - Call state machine
- `@/stores/settings` - User preferences

**Local State (createSignal):**

- UI-only state (modals open, loading)
- Form inputs (uncontrolled)
- Hover/focus states

**Derived State (createMemo):**

- Computed values from stores
- Filtered lists
- Grouped messages

### Data Flow

```
WebSocket Events → Store Actions → Store State Updates → Component Re-renders
Tauri Commands → Store Actions → Store State Updates → Component Re-renders
User Interactions → Event Handlers → Store Actions or Tauri Commands
```

## Styling System

### UnoCSS (Tailwind-Compatible Utilities)

```tsx
<div class="px-4 py-2 bg-surface-base text-text-primary rounded-lg">
```

### CSS Custom Properties (Theming)

```tsx
<div style="background-color: var(--color-accent-primary)">
```

### Dynamic Classes (Solid classList)

```tsx
<div
  class="base-class"
  classList={{
    "active": isActive(),
    "disabled": isDisabled(),
  }}
>
```

### Design Tokens

Located in `client/src/styles/design-tokens.css`:

- Colors: `--color-{category}-{variant}`
- Spacing: Tailwind scale (0.5 → 24)
- Typography: Inter font, sizes xs → 2xl
- Borders: `border-white/10` for subtle dividers
- Shadows: `shadow-{size}` for elevation

## Performance Targets

### Client Performance Goals

- Idle RAM: <80MB (vs Discord ~400MB)
- Idle CPU: <1%
- Startup: <3s
- Message render: <16ms per message
- Voice latency: <50ms end-to-end

### Optimization Strategies

- Lazy load modals (dynamic imports)
- Virtualize long lists (messages, channels)
- Memoize expensive computations
- Debounce/throttle event handlers
- Efficient WebSocket event handling
- Solid.js fine-grained reactivity

## Component Guidelines

### File Organization

```
component-name/
├── ComponentName.tsx    # Main component
├── SubComponent.tsx     # Sub-components (if complex)
├── index.ts             # Re-exports for clean imports
└── AGENTS.md            # Documentation (you are here)
```

### Import Conventions

```tsx
// External libraries
import { Component, createSignal } from "solid-js";

// Tauri APIs
import { invoke } from "@tauri-apps/api/core";

// Stores
import { authState } from "@/stores/auth";

// Components
import Avatar from "@/components/ui/Avatar";

// Utils
import { formatDate } from "@/lib/utils";

// Types
import type { User } from "@/lib/types";
```

### Component Template

```tsx
import { Component } from "solid-js";

interface MyComponentProps {
  // Props here
}

const MyComponent: Component<MyComponentProps> = (props) => {
  // Local state
  const [state, setState] = createSignal(initialValue);

  // Event handlers
  const handleClick = () => {
    // ...
  };

  // Render
  return <div class="...">{/* Content */}</div>;
};

export default MyComponent;
```

### Accessibility

- Use semantic HTML (button, input, nav, etc.)
- Add ARIA labels where needed
- Support keyboard navigation
- Announce dynamic content to screen readers
- Maintain focus management in modals

### Testing (Future)

- Unit tests for business logic
- Component tests for user interactions
- Snapshot tests for visual regression
- E2E tests for critical flows (login, send message, voice join)

## Integration with Backend

### Tauri Commands (Client → Server)

```tsx
import { invoke } from "@tauri-apps/api/core";

// Example: Send message
await invoke("send_message", {
  channelId: "uuid",
  content: "Hello world",
});
```

### WebSocket Events (Server → Client)

Handled in stores, trigger component updates:

```tsx
// Store listens to WebSocket
onWebSocketMessage((event) => {
  if (event.type === "message.created") {
    addMessage(event.payload);
  }
});
```

### Tauri Event Bus (Rust → Frontend)

```tsx
import { listen } from "@tauri-apps/api/event";

// Listen for voice events from Rust core
listen("voice:speaking", (event) => {
  updateSpeaking(event.payload.userId, true);
});
```

## Common Patterns

### Modal Pattern

```tsx
const [showModal, setShowModal] = createSignal(false);

// In render
<Show when={showModal()}>
  <MyModal onClose={() => setShowModal(false)} />
</Show>;
```

### List Rendering

```tsx
<For each={items()}>{(item) => <ItemComponent item={item} />}</For>
```

### Conditional Rendering

```tsx
<Show when={condition()} fallback={<LoadingSpinner />}>
  <Content />
</Show>
```

### Form Handling

```tsx
const [formData, setFormData] = createSignal({ name: "" });

const handleSubmit = (e: Event) => {
  e.preventDefault();
  // Submit logic
};

<form onSubmit={handleSubmit}>
  <input
    value={formData().name}
    onInput={(e) => setFormData({ name: e.currentTarget.value })}
  />
</form>;
```

## Future Improvements

### Component Enhancements

- Storybook for component documentation
- Automated visual regression tests
- Component prop type generation
- Animation system (Framer Motion or Solid Transition Group)

### Performance

- Virtual scrolling for all lists
- Image lazy loading
- Code splitting per route
- Service worker for offline support

### Features

- Rich text editor for messages (mention autocomplete, emoji picker)
- Drag-and-drop file uploads
- Screen sharing UI
- Video call layouts
- Mobile responsive design

## Related Documentation

- Design system: `docs/design-system.md` (if exists)
- Solid.js patterns: `docs/solidjs-patterns.md` (if exists)
- Store architecture: `client/src/stores/AGENTS.md`
- Tauri integration: `docs/tauri-integration.md` (if exists)
- WebSocket protocol: `STANDARDS.md` § WebSocket
