# Home Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Home view (DM-focused interface) when no guild is selected. Three-column layout with DM sidebar, content area, and context panel.

## Key Files

### HomeView.tsx

Main layout orchestrator for home view.

**Layout Structure:**

```
┌────────────┬─────────────────┬──────────────┐
│ DM Sidebar │ Content Area    │ Right Panel  │
│ (DMSidebar)│ (Friends or DM) │ (Context)    │
└────────────┴─────────────────┴──────────────┘
```

**Content Switching:**

- `dmsState.isShowingFriends === true` → Show FriendsList
- `dmsState.isShowingFriends === false` → Show DMConversation

**Usage:**

```tsx
import HomeView from "@/components/home/HomeView";

// When guildsState.activeGuildId is null
<HomeView />;
```

### DMSidebar.tsx

Left sidebar with DM conversations and Friends button.

**Expected Features:**

- "Friends" button at top (shows friends list)
- List of DM conversations
- "New Message" button
- User search/filter

**State:**

- Highlights active DM or Friends view
- Shows unread message counts (badges)

### DMConversation.tsx

Active DM conversation view (replaces channel view).

**Expected Components:**

- DM header (participant names, call button)
- Message list
- Message input
- CallBanner integration

**Usage:**

- Shown when DM selected and `!isShowingFriends`

### DMItem.tsx

Individual DM conversation row in sidebar.

**Expected Display:**

- Participant avatar(s)
- Last message preview
- Timestamp
- Unread badge
- Online status indicator

### HomeRightPanel.tsx

Right context panel (responsive, hidden on small screens).

**Expected Content:**

- Active call participants (if in call)
- Friend requests (pending)
- Online friends quick list
- Activity feed (future)

### NewMessageModal.tsx

Modal for starting new DM conversation.

**Features:**

- User search (by username)
- Friend quick-select
- Multi-select for group DMs (future)

## State Management

### From Stores

- `dmsState.conversations` - All DM conversations
- `dmsState.activeConversationId` - Selected DM
- `dmsState.isShowingFriends` - Friends view toggle
- `friendsState.pendingRequests` - Friend request count
- `callState.currentCall` - Active call info

## Integration Points

### Components

- `FriendsList` (from `@/components/social`) - Friends interface
- `CallBanner` (from `@/components/call`) - Call status
- `MessageList` (from `@/components/messages`) - Message display
- `MessageInput` (from `@/components/messages`) - Send messages

### Stores

- `@/stores/dms` - DM conversations and selection
- `@/stores/friends` - Friends and requests
- `@/stores/call` - Call state

## Navigation Flow

1. **User clicks "Friends" button:**
   - `setIsShowingFriends(true)`
   - Content area switches to FriendsList
   - DM selection cleared

2. **User clicks DM conversation:**
   - `setIsShowingFriends(false)`
   - `setActiveConversation(conversationId)`
   - Content area switches to DMConversation

3. **User clicks "New Message":**
   - Opens NewMessageModal
   - On user select → creates/opens DM conversation

## Responsive Behavior

### Three-Column Layout

- **Desktop (>1200px):** All three columns visible
- **Tablet (768px-1200px):** Hide right panel
- **Mobile (<768px):** Hide sidebar when conversation open

### Expected Breakpoints

```css
/* Full layout */
@media (min-width: 1200px) {
  /* show all */
}

/* Hide right panel */
@media (max-width: 1199px) {
  /* hide HomeRightPanel */
}

/* Stack layout */
@media (max-width: 767px) {
  /* hide sidebar when DM active */
}
```

## DM Types

### One-on-One DMs

- Single participant
- Standard message exchange
- Voice/video calls

### Group DMs (Future)

- Multiple participants
- Group call support
- Admin/creator permissions

## Empty States

### No DMs

- Show welcome message
- Prompt to add friends or start conversation
- Quick actions (Add Friend, Join Guild)

### Friends View

- Show "Add Friend" if no friends
- Display online friends prominently

## Future Enhancements

- DM folders/categories
- Pinned conversations
- Mute/notification settings per DM
- DM search
- Rich presence in right panel
- Message previews with formatting
- Typing indicators in sidebar

## Related Documentation

- DM system: `PROJECT_SPEC.md` § Direct Messages
- WebSocket events: `STANDARDS.md` § WebSocket Protocol
- Message storage: `ARCHITECTURE.md` § Chat Service
