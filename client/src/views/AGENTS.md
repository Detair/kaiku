<!-- Parent: ../AGENTS.md -->

# views

## Purpose

Top-level authenticated and unauthenticated views. Each view represents a complete application screen or major workflow.

## Key Files

- `Main.tsx` - Primary application interface (authenticated users)
- `Login.tsx` - Login form view
- `Register.tsx` - Registration form view
- `InviteJoin.tsx` - Guild invite acceptance flow

## For AI Agents

### View Hierarchy

```
App.tsx (root router)
├── Login.tsx (unauthenticated)
├── Register.tsx (unauthenticated)
├── InviteJoin.tsx (can be unauthenticated or authenticated)
└── Main.tsx (authenticated)
    └── AppShell layout
        ├── ServerRail (guild switcher)
        ├── Sidebar (channels + user panel)
        ├── Main content area
        │   ├── HomeView (DMs/Friends when no guild selected)
        │   └── Channel view (messages when channel selected)
        ├── VoiceIsland (floating voice controls)
        └── CommandPalette (Ctrl+K quick actions)
```

### Main.tsx

The core application view:

- Uses AppShell component for layout structure
- Shows HomeView when `activeGuildId === null`
- Shows channel messages when channel selected
- Includes global CommandPalette
- Loads guilds on mount
- Handles empty states (no channel selected)

### Login.tsx / Register.tsx

Authentication forms:

- Call `@/stores/auth` actions (login, register)
- Auto-redirect to Main on success
- Show validation errors inline
- Server URL input (self-hosted support)
- Remember server URL in localStorage

### InviteJoin.tsx

Invite code handler:

- Parses invite code from URL (/invite/:code)
- Shows guild preview before joining
- Handles both authenticated and unauthenticated users
- Redirects to login if needed
- Joins guild and redirects to Main on success

### Layout Composition

Views use composition over props:

- `Main.tsx` composes AppShell + content components
- No layout prop drilling
- Each view owns its full screen layout

### Empty States

Views handle their own empty states:

- Main: "Select a channel" when none selected
- HomeView: "Add friends" when no DMs
- Proper icons and helper text for discoverability

### Data Loading

Views trigger data loading on mount:

```typescript
onMount(async () => {
  await loadGuilds();
  await loadChannels();
});
```

### Navigation Guards

Router handles auth checks:

- Redirect to /login if not authenticated
- Redirect to /app if already authenticated (on login/register views)
- See App.tsx for routing logic
