# Unified UI Features Design

**Date:** 2026-01-24
**Status:** Design Approved
**Features:** Voice Quality Indicators, User Presence, Message Reactions, Channel Categories

---

## Overview

Four features designed with a unified visual language:
1. **Voice Quality Indicators** - Real-time connection stats for voice participants
2. **User Presence & Status** - Online/idle/DND/invisible + activities
3. **Message Reactions & Emoji** - Twemoji + guild custom emojis
4. **Channel Categories** - 2-level folder hierarchy for channels

---

## Shared Design System

### Display Modes

User-configurable via server-synced preferences:

```typescript
type DisplayMode = 'dense' | 'minimal' | 'discord';

interface DisplayPreferences {
  indicatorMode: DisplayMode;
  showLatencyNumbers: boolean;
  reactionStyle: 'bar' | 'compact';
}
```

- **Dense (default):** Full information visible (stats, text, counts)
- **Minimal:** Icons/shapes only, details on hover
- **Discord:** Familiar Discord-like patterns

### Accessibility Shape System

Shapes provide meaning independent of color for color-blind users:

| Quality/Status | Shape | Color | Hex |
|----------------|-------|-------|-----|
| Good / Online | ● Circle | Green | `#23a55a` |
| Warning / Idle | ▲ Triangle | Yellow | `#f0b232` |
| Poor / DND | ⬡ Hexagon | Red | `#f23f43` |
| Offline / Unknown | ○ Empty circle | Gray | `#80848e` |
| Streaming | ● Circle | Purple | `#593695` |

### Animation Guidelines

- Duration: 150ms ease-out
- Style: Subtle fades only
- No bouncing, scaling, or attention-grabbing motion
- Status changes crossfade, don't pop
- Reactions fade in at point of click

### DND Behavior

- **Suppresses:** Notification sounds, toast popups, desktop notifications
- **Keeps active:** Unread counters, badge numbers, in-app indicators
- **Exception:** Direct mentions from server owner (configurable)

### Rate Limits

| Action | Limit | Rationale |
|--------|-------|-----------|
| Status change | 1 per 10s | Prevents status flickering |
| Custom status text | 1 per 30s | Prevents spam |
| Reaction add/remove | 5 per 3s per user | Allows quick multi-react |
| Unique reactions per message | 20 max | Keeps UI manageable |
| Same reaction per message | 1 per user | Can't spam same emoji |
| Emoji upload (guild) | 5 per hour | Prevents abuse |

---

## Feature 1: Voice Quality Indicators

### Data Model

```typescript
interface ConnectionMetrics {
  latency: number;      // RTT in ms
  packetLoss: number;   // 0-100%
  jitter: number;       // ms
  quality: 'good' | 'warning' | 'poor' | 'unknown';
  timestamp: number;
}
```

### Quality Thresholds

| Quality | Latency | Packet Loss | Jitter | Shape |
|---------|---------|-------------|--------|-------|
| Good | <100ms | <1% | <30ms | ● Circle |
| Warning | 100-300ms | 1-5% | 30-60ms | ▲ Triangle |
| Poor | >300ms | >5% | >60ms | ⬡ Hexagon |

Quality determined by **worst metric**.

### UI Placements

**VoiceIsland (bottom left panel)**
```
┌─────────────────────────────────────┐
│ 🎤 General Voice  │ ● 42ms │ ⚙️ ✕  │
└─────────────────────────────────────┘
```

**Participant List**
```
┌──────────────────────────────────┐
│ 👤 Alice              ● 38ms 🎤 │
│ 👤 Bob                ▲ 142ms 🔇│
│ 👤 You                ● 42ms 🎤 │
└──────────────────────────────────┘
```

**Quality Tooltip (on hover)**
```
┌────────────────────────────────┐
│ Connection Quality             │
├────────────────────────────────┤
│ Latency      42ms     ●        │
│ Packet Loss  0.3%     ●        │
│ Jitter       58ms     ▲ ← worst│
├────────────────────────────────┤
│ Overall: Good                  │
└────────────────────────────────┘
```

### Notifications

- Warning toast at 3% packet loss (auto-dismiss 5s)
- Critical toast at 7% packet loss (persists until recovery)
- 10s cooldown between incidents
- Respects DND (no toast, indicator still updates)

### Data Flow

1. Client extracts WebRTC stats every 3s
2. Sends to server via WebSocket
3. Server broadcasts to room participants
4. Server stores in TimescaleDB for history

---

## Feature 2: User Presence & Status

### Status Types

| Status | Color | Shape | Auto-Set | User-Set |
|--------|-------|-------|----------|----------|
| Online | Green | ● | On connect | ✓ |
| Idle | Yellow | ▲ | After 5min inactivity | ✓ |
| Do Not Disturb | Red | ⬡ | — | ✓ |
| Invisible | Gray | ○ | — | ✓ |
| Offline | Gray | ○ | On disconnect | — |

### Activity Types

| Type | Display | Icon | Source |
|------|---------|------|--------|
| Playing | "Playing Minecraft" | 🎮 | Process detection |
| Streaming | "Streaming on Twitch" | 📺 | Screen share active |
| Listening | "Listening to Spotify" | 🎵 | Process detection |
| Watching | "Watching YouTube" | 📺 | Future: browser ext |
| Custom | User-defined text | 💬 | Manual |

### Data Model

```typescript
interface UserPresence {
  status: 'online' | 'idle' | 'dnd' | 'invisible' | 'offline';
  customStatus?: {
    text: string;        // "In a meeting"
    emoji?: string;      // "📅"
    expiresAt?: string;  // ISO timestamp
  };
  activity?: {
    type: 'playing' | 'streaming' | 'listening' | 'watching' | 'custom';
    name: string;        // "Minecraft"
    details?: string;    // "Creative Mode"
    startedAt: string;
  };
}
```

### UI Placements

**Member List Item**
```
┌────────────────────────────────────┐
│ [●] Alice                          │
│     🎮 Playing Minecraft           │
├────────────────────────────────────┤
│ [▲] Bob                            │
│     📅 In a meeting until 3pm      │
├────────────────────────────────────┤
│ [⬡] Charlie                        │
│     Do Not Disturb                 │
└────────────────────────────────────┘
```

**User Popover**
```
┌──────────────────────────────────────┐
│ [Avatar ●]  Alice                    │
│             @alice                   │
├──────────────────────────────────────┤
│ 🎮 PLAYING A GAME                    │
│    Minecraft                         │
│    for 2 hours                       │
├──────────────────────────────────────┤
│ 📅 In a meeting until 3pm            │
├──────────────────────────────────────┤
│ [Message] [Call] [Add Friend]        │
└──────────────────────────────────────┘
```

**Status Picker**
```
┌──────────────────────────────────────┐
│ Set Status                           │
├──────────────────────────────────────┤
│ ● Online                             │
│ ▲ Idle                               │
│ ⬡ Do Not Disturb                     │
│ ○ Invisible                          │
├──────────────────────────────────────┤
│ 💬 Set Custom Status...              │
│ ┌────────────────────────────────┐   │
│ │ 📅 In a meeting until 3pm     │   │
│ └────────────────────────────────┘   │
│ Clear after: [4 hours ▼]             │
└──────────────────────────────────────┘
```

### Idle Detection

- Tracks mouse/keyboard activity
- After 5 minutes inactivity → auto-set Idle
- On activity resume → restore previous status
- Configurable timeout (1-30 min, or disable)

### Privacy Controls

- Toggle: "Share what I'm doing"
- Per-app visibility list
- Invisible mode: appear offline, still use app

---

## Feature 3: Message Reactions & Emoji

### Data Model

```typescript
interface Reaction {
  emoji: string;           // Unicode or custom emoji ID
  count: number;
  users: string[];         // User IDs who reacted
  me: boolean;             // Did current user react
}

interface CustomEmoji {
  id: string;
  name: string;            // :pepe_laugh:
  guildId: string;
  imageUrl: string;
  animated: boolean;
  uploadedBy: string;
  createdAt: string;
}
```

### Emoji Sources (Priority Order)

1. Recent emojis (last 20 used)
2. Favorites (user-pinned)
3. Guild custom emojis
4. Twemoji full set (searchable)

### UI Components

**Reaction Bar (below message)**
```
┌────────────────────────────────────────────────┐
│ Alice: Hey, check out this screenshot!         │
│ [image.png]                                    │
│                                                │
│ [😂 3] [🔥 2] [👍 1] [+]                       │
└────────────────────────────────────────────────┘
```

**Reaction Tooltip (hover)**
```
┌──────────────────┐
│ 😂               │
│ Alice, Bob, You  │
└──────────────────┘
```

**Emoji Picker**
```
┌────────────────────────────────────────────┐
│ 🔍 Search emoji...                         │
├────────────────────────────────────────────┤
│ RECENT                                     │
│ 😂 🔥 👍 ❤️ 🎉 👀 🚀 ✅               │
├────────────────────────────────────────────┤
│ FAVORITES                    [Edit]        │
│ ⭐ 💯 🙌 🤔                               │
├────────────────────────────────────────────┤
│ GUILD EMOJIS - Wolftown                    │
│ :pepe: :kekw: :sadge: :pog:               │
├────────────────────────────────────────────┤
│ 😀 SMILEYS & EMOTION                       │
│ 😀😃😄😁😆😅🤣😂🙂🙃😉😊              │
└────────────────────────────────────────────┘
```

**Quick Reactions (hover message)**
```
┌──────────────────────────────────────────┐
│ Alice: Hey everyone!     [😂][👍][❤️][+]│
└──────────────────────────────────────────┘
```

### Features

- Search by name and keywords
- Skin tone selector (long-press)
- Recent + favorites persisted
- Guild emoji management (upload/delete)

### Permissions

- `MANAGE_EMOJIS` - Upload/delete guild emojis
- `ADD_REACTIONS` - React to messages (default: everyone)
- `USE_EXTERNAL_EMOJIS` - Use emojis from other guilds (future)

---

## Feature 4: Channel Categories

### Data Model

```typescript
interface ChannelCategory {
  id: string;
  guildId: string;
  name: string;
  position: number;
  parentId: string | null; // null = top-level
  collapsed: boolean;      // User's local state
  permissionOverrides: PermissionOverride[];
}

interface Channel {
  // ... existing fields
  categoryId: string | null;
  position: number;
}
```

### Hierarchy Rules

- Max 2 levels: Category → Subcategory → Channels
- Subcategories cannot contain subcategories
- Channels can exist at root or inside any category
- Categories can be empty

### UI - Channel Sidebar

```
┌────────────────────────────────────┐
│ 🏠 Wolftown Gaming                 │
├────────────────────────────────────┤
│ ▼ TEXT CHANNELS                    │
│   # general                        │
│   # announcements                  │
│   ▼ Game Discussions               │
│      # minecraft                   │
│      # valorant                    │
│   ▶ Archives (collapsed)           │
├────────────────────────────────────┤
│ ▼ VOICE CHANNELS                   │
│   🔊 General Voice                 │
│   ▼ Private Rooms                  │
│      🔊 Team Alpha                 │
│      🔊 Team Beta                  │
├────────────────────────────────────┤
│ # uncategorized-channel            │
└────────────────────────────────────┘
```

### Visual Indicators

- `▼` Expanded (clickable)
- `▶` Collapsed (clickable)
- Subcategories indented with left border
- Category names: ALL CAPS, muted color
- Subcategory names: Title Case

### Interactions

- Click header to collapse/expand
- Collapse state saved locally
- Collapsed shows unread indicator
- Drag & drop reordering
- Context menu: Edit, Create Channel, Create Subcategory, Delete

### Permission Inheritance

- Channels inherit category permissions by default
- Can override at channel level
- UI shows "Synced" or "Custom permissions"

### Permissions

- `MANAGE_CHANNELS` - Create/edit/delete categories and channels

---

## Implementation Notes

### Database Changes

- Add `reactions` table (message_id, emoji, user_id)
- Add `custom_emojis` table (guild_id, name, image_url, etc.)
- Add `categories` table (guild_id, name, position, parent_id)
- Add `category_id` to channels table
- Extend presence with activity fields

### New Components

- `<StatusIndicator>` - Reusable shape+color indicator
- `<QualityTooltip>` - Connection breakdown
- `<EmojiPicker>` - Full picker with search
- `<ReactionBar>` - Message reactions display
- `<CategoryHeader>` - Collapsible category
- `<StatusPicker>` - User status selection
- `<UserPopover>` - Enhanced user card

### API Endpoints

**Reactions:**
- `PUT /channels/:id/messages/:id/reactions/:emoji` - Add reaction
- `DELETE /channels/:id/messages/:id/reactions/:emoji` - Remove reaction

**Emojis:**
- `GET /guilds/:id/emojis` - List guild emojis
- `POST /guilds/:id/emojis` - Upload emoji
- `DELETE /guilds/:id/emojis/:id` - Delete emoji

**Categories:**
- `POST /guilds/:id/categories` - Create category
- `PATCH /guilds/:id/categories/:id` - Update category
- `DELETE /guilds/:id/categories/:id` - Delete category

**Presence:**
- WebSocket: `presence_update` event extended with activity

---

## Success Criteria

1. Voice quality visible in VoiceIsland and participant list
2. Users can set status and custom status with expiry
3. Activity detection shows current game/app
4. Users can react to messages with emoji picker
5. Guild admins can upload custom emojis
6. Channels organized in collapsible categories
7. All features respect DND mode
8. Accessibility shapes work for color-blind users
9. Display mode preference syncs across devices
