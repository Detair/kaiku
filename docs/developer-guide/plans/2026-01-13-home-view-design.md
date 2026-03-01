# Home View Design

## Summary

Three-column Discord-style layout for the Home view (when no guild is selected). Includes DM list sidebar, main content area (Friends or DM conversation), and context-aware right panel.

## Layout

```
┌────────────────────────────────────────────────────────────────┐
│  ServerRail │ DMSidebar      │ HomeContent       │ RightPanel  │
│  (existing) │ (240px)        │ (flex-1)          │ (240px)     │
│             │                │                   │             │
│  [Home] ●   │ [Friends]      │ FriendsList OR    │ Context-    │
│  [Guild1]   │ [+ New Message]│ DMConversation    │ aware       │
│  [Guild2]   │ ─────────────  │                   │ panel       │
│  [+]        │ DM List...     │                   │             │
└────────────────────────────────────────────────────────────────┘
```

## Component Structure

```
HomeView (container)
├── DMSidebar (left column - 240px)
│   ├── TabBar (Friends tab)
│   ├── NewMessageButton
│   └── DMList (scrollable)
│       └── DMItem[] (avatar + name + preview + unread)
├── HomeContent (middle column - flex-1)
│   ├── When Friends tab: FriendsList (existing)
│   └── When DM selected: DMConversation
│       ├── DMHeader
│       ├── MessageList (existing)
│       ├── TypingIndicator (existing)
│       └── MessageInput (existing)
└── HomeRightPanel (right column - 240px, conditional)
    ├── When Friends view: Empty or OnlineFriendsCount
    ├── When 1:1 DM: UserProfilePanel
    └── When Group DM: ParticipantsPanel
```

## State Management

### DMs Store (`client/src/stores/dms.ts`)

```typescript
interface DMsStoreState {
  dms: DMChannel[];              // DMs from API (includes last_message)
  selectedDMId: string | null;
  isShowingFriends: boolean;
  typingUsers: Record<string, string[]>;
  isLoading: boolean;
}

// Actions
loadDMs(): Promise<void>
selectDM(id: string): void
selectFriendsTab(): void
updateDMLastMessage(channelId: string, message: Message): void
handleDMRead(channelId: string): void
```

### Key Principles

1. **Server is source of truth** for unread counts and last messages
2. **WebSocket updates** for real-time sync across devices
3. **Subscribe to all DMs** on Home view load
4. **Debounce read marking** (1s delay before API call)

## Cross-Device Read State Sync

When a user reads messages on device 1, device 2 should update immediately.

### Database

```sql
CREATE TABLE dm_read_state (
  user_id UUID REFERENCES users(id),
  channel_id UUID REFERENCES channels(id),
  last_read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  last_read_message_id UUID REFERENCES messages(id),
  PRIMARY KEY (user_id, channel_id)
);
```

### API

```
GET /api/dm
  - Returns: DMChannel[] with last_message, unread_count per DM

POST /api/dm/:channel_id/read
  - Updates last_read_at for user+channel
  - Broadcasts "dm_read" to ALL user's WebSocket sessions
  - Returns new unread count (0)
```

### WebSocket Event

```typescript
// Server → All user sessions
{
  type: "dm_read",
  channel_id: string,
  last_read_at: string,
  last_read_message_id: string
}
```

### Flow

1. User opens DM on device 1 → client calls `POST /api/dm/:id/read`
2. Server updates `dm_read_state` table
3. Server broadcasts `dm_read` to ALL sessions for this user
4. Device 2 receives event → updates unread_count to 0
5. Badge disappears on device 2

## UI Components

### DMSidebar

```
┌─────────────────────────┐
│ [Friends]               │ ← Tab (accent when active)
├─────────────────────────┤
│ [+ New Message]         │ ← Opens NewMessageModal
├─────────────────────────┤
│ ┌─────────────────────┐ │
│ │ 🟢 Alice            │ │ ← Online indicator
│ │ Hey, are you free?  │ │ ← Last message preview
│ │             2m ago  │ │ ← Relative timestamp
│ └─────────────────────┘ │
│ ┌─────────────────────┐ │
│ │ 🔴 Bob         (3)  │ │ ← Unread badge
│ │ Check this out!     │ │
│ └─────────────────────┘ │
│ ┌─────────────────────┐ │
│ │ 👥 Gaming Squad     │ │ ← Group DM icon
│ │ Charlie: lol        │ │ ← Shows author
│ └─────────────────────┘ │
└─────────────────────────┘
```

### DMItem

- Avatar with online status dot (1:1 DMs)
- Name + optional unread badge
- Last message preview (truncated)
- Author prefix for group DMs ("You:", "Alice:")
- Relative timestamp
- Typing indicator replaces preview
- Selected state: highlighted background

### HomeRightPanel

**Friends tab active:**
- Empty or minimal (e.g., "Online — 3")

**1:1 DM selected:**
- Large avatar
- Display name + username
- Member since date
- Mutual guilds list
- Actions: Block, Remove Friend

**Group DM selected:**
- Group name
- Member count
- Participant list with online status
- Actions: Add People, Leave Group

### NewMessageModal

- Search/filter friends by name
- Checkboxes for multi-select
- Shows selected count
- "Create DM" button
- Reuses existing DM if one exists for 1:1

## Starting a New DM

Two entry points:
1. **"+ New Message" button** in DMSidebar → Opens NewMessageModal
2. **"Message" button** on FriendItem → Creates/opens 1:1 DM directly

## Files to Create

### Frontend

```
client/src/components/home/
├── HomeView.tsx
├── DMSidebar.tsx
├── DMItem.tsx
├── DMConversation.tsx
├── HomeRightPanel.tsx
├── UserProfilePanel.tsx
├── ParticipantsPanel.tsx
├── NewMessageModal.tsx
└── index.ts

client/src/stores/
└── dms.ts
```

### Backend

```
server/migrations/
└── NNNN_add_dm_read_state.sql

server/src/chat/
└── dm.rs (modify)

server/src/ws/
└── mod.rs (add dm_read broadcast)
```

## Files to Modify

- `client/src/views/Main.tsx` - Use HomeView instead of FriendsList
- `client/src/components/social/FriendsList.tsx` - Add "Message" button
- `server/src/chat/dm.rs` - Add last_message, unread_count to GET /api/dm
- `server/src/chat/dm.rs` - Add POST /api/dm/:id/read endpoint

## Implementation Order

1. Database migration for `dm_read_state`
2. Backend: Modify GET /api/dm response
3. Backend: Add POST /api/dm/:id/read endpoint
4. Backend: WebSocket dm_read broadcast
5. Frontend: dms.ts store
6. Frontend: DMSidebar + DMItem components
7. Frontend: HomeView container
8. Frontend: Right panel components
9. Frontend: NewMessageModal
10. Integration + testing

## Responsive Behavior

- Right panel hidden when viewport < 1200px
- Right panel can be toggled via header button
- DMSidebar collapses to icons only at < 768px (future enhancement)

## Performance Targets

- DM list render: < 50ms for 100 DMs
- Read state sync: < 200ms cross-device
- Typing indicator: < 100ms latency
