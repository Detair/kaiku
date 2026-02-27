# Layout Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Application shell and primary layout structure. Implements "The Focused Hybrid" design philosophy combining Discord's structure with Linear/Arc efficiency.

## Key Files

### AppShell.tsx

Main layout grid with four zones.

**Layout Zones:**

```
┌──────┬─────────┬──────────────────┐
│Server│ Context │   Main Stage     │
│ Rail │ Sidebar │   (flex-1)       │
│(72px)│ (240px) │                  │
└──────┴─────────┴──────────────────┘
         └─ Voice Island (overlay) ─┘
```

**Zone Descriptions:**

1. **Server Rail** (leftmost, 72px) - Guild/server switcher
2. **Context Sidebar** (240px) - Channels, DMs, user panel
3. **Main Stage** (flex-1) - Chat messages, content
4. **Voice Island** (overlay) - Floating draggable voice controls

**Props:**

- `showServerRail?: boolean` - Toggle server rail visibility (default: false)

**Usage:**

```tsx
import AppShell from "@/components/layout/AppShell";

<AppShell showServerRail={true}>
  {/* Main content (stage area) */}
  <GuildView /> or <HomeView />
</AppShell>;
```

**Conditional Rendering:**

- Voice Island only shown when `voiceState.channelId` exists

### ServerRail.tsx

Leftmost vertical bar for guild/server navigation.

**Expected Features:**

- Home button (DM view)
- Guild icons (scrollable list)
- Add/Join Guild button
- Guild unread indicators

**Visual Design:**

- 72px fixed width
- Vertical icon stack
- Pill-shaped selection indicator
- Subtle hover states

### Sidebar.tsx

Context-aware middle sidebar.

**Content Modes:**

- **Guild Mode:** ChannelList + UserPanel
- **Home Mode:** DMSidebar + UserPanel

**Structure:**

```tsx
<div class="w-60 flex flex-col">
  {/* Top: Content (flex-1) */}
  <ChannelList /> or <DMSidebar />
  {/* Bottom: User Panel (fixed) */}
  <UserPanel />
</div>
```

**Styling:**

- 240px (w-60) fixed width
- Background: `--color-surface-base`
- Border: `border-r border-white/10`

### UserPanel.tsx

Bottom panel with user info and controls.

**Expected Display:**

- User avatar with status indicator
- Username and status message
- Mute/deafen buttons (when in voice)
- Settings button
- Logout button

**Actions:**

- Click avatar → Open user settings
- Click settings icon → Open SettingsModal
- Mute/deafen → Toggle voice state

### VoiceIsland.tsx

Floating overlay for voice controls (when in voice channel).

**Features:**

- Draggable positioning
- Compact voice controls
- Participant list
- Channel name display
- Leave channel button

**Behavior:**

- Appears when joining voice channel
- Persists across view changes
- User-positioned via drag
- Saves position to localStorage

**Default Position:**

- Bottom-right corner
- Above main content
- Z-index: 40

### CommandPalette.tsx

Quick action overlay (Cmd+K / Ctrl+K).

**Expected Features:**

- Search channels/DMs/users
- Quick navigation
- Command execution (mute, deafen, status)
- Recent/frequent actions

**Keyboard:**

- `Cmd/Ctrl + K` - Open
- `ESC` - Close
- Arrow keys - Navigate
- Enter - Execute

## Layout Philosophy

### The Focused Hybrid

- **From Discord:** Familiar structure (rail, sidebar, main)
- **From Linear/Arc:** Efficiency (Cmd+K, minimal chrome, polish)
- **Goal:** <80MB RAM, <1% CPU idle

### Responsive Strategy

Currently desktop-first. Future responsive breakpoints:

- Desktop (>1024px): Full layout
- Tablet (768px-1024px): Hide server rail
- Mobile (<768px): Stack layout, slide-out sidebar

## Integration Points

### Components

- `ChannelList` (from `@/components/channels`) - Guild channels
- `DMSidebar` (from `@/components/home`) - DM conversations
- `VoiceControls` (from `@/components/voice`) - Voice UI

### Stores

- `voiceState.channelId` - Show/hide Voice Island
- `guildsState.activeGuildId` - Sidebar mode (guild vs home)

## Styling System

### CSS Variables

```css
--color-surface-base: Sidebar background --color-surface-layer1: Main stage
  background --color-surface-layer2: Elevated elements;
```

### Spacing

- Server Rail: 72px (18 in Tailwind)
- Sidebar: 240px (60 in Tailwind)
- Main Stage: flex-1 (dynamic)

### Z-Index Layers

- Base: 0 (main content)
- Sidebar: 10
- Voice Island: 40
- Modals: 50
- Command Palette: 60

## Performance Considerations

### Layout Shifts

- Fixed widths prevent layout shift
- Flex-1 main stage absorbs resize
- Voice Island uses fixed positioning

### Render Optimization

- Voice Island only mounts when needed
- Server Rail conditionally rendered
- Children passed to AppShell (not re-created)

## Future Enhancements

- Resizable sidebar (drag border)
- Collapsible server rail
- Picture-in-picture voice island
- Multi-window support (Tauri)
- Custom layouts per guild
- Zen mode (hide sidebars)

## Related Documentation

- Design philosophy: `docs/design-philosophy.md` (if exists)
- Layout performance: `ARCHITECTURE.md` § Client Performance
- Voice Island UX: `docs/voice-ux.md` (if exists)
