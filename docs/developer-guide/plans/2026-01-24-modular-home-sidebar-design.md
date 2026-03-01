# Modular Home Sidebar - Design

> **Status:** Approved
> **Date:** 2026-01-24

## Overview

Add collapsible modules to the Home right panel for quick access to pending items, personal pins, and future extensibility.

---

## MVP Scope

- Collapsible module framework (fixed order, no drag-drop)
- Module order: Active Now → Pending → Pins
- Two new modules: Pending & Suggestions, Global Pins
- Server Pulse deferred to future PR

---

## Module Framework

### Layout
- Right panel (360px) contains vertical stack of collapsible modules
- Each module has header (title + collapse toggle) and content area
- Collapsed modules show only header with item count badge
- Smooth expand/collapse animation (150ms)

### Preferences
Add to `UserPreferences.homeSidebar`:
```typescript
homeSidebar: {
  collapsed: {
    activeNow: boolean;
    pending: boolean;
    pins: boolean;
  };
}
```

### Component Structure
```
HomeRightPanel
├── CollapsibleModule (generic wrapper)
│   ├── ModuleHeader (title, badge, collapse toggle)
│   └── ModuleContent (children)
├── ActiveNowModule (existing, refactored)
├── PendingModule (new)
└── PinsModule (new)
```

---

## Pending & Suggestions Module

### Content
- **Incoming friend requests** - Avatar, name, Accept/Decline buttons
- **Outgoing friend requests** - Avatar, name, Cancel button
- **Guild invites** - Guild icon, name, Accept/Decline buttons
- **Suggestions** (when no pending items):
  - "Add friends by username" prompt
  - Link to discover communities (future)

### Data Sources
- Friend requests: `friendsState.pendingIncoming`, `friendsState.pendingOutgoing`
- Guild invites: Check existing API or add if needed

### UI Behavior
- Header shows count badge: "Pending (3)"
- Empty state shows suggestion prompts
- Real-time updates via existing WebSocket events

---

## Global Pins Module

### Content Types
- **Notes** - Markdown text, max 2000 chars
- **Links** - URL + optional title
- **Pinned messages** - Reference to channel message with preview

### Database Table
```sql
CREATE TABLE user_pins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    pin_type VARCHAR(20) NOT NULL, -- 'note', 'link', 'message'
    content TEXT NOT NULL,         -- note text, URL, or message_id
    title VARCHAR(255),            -- optional title
    metadata JSONB DEFAULT '{}',   -- channel_id for messages, etc.
    created_at TIMESTAMPTZ DEFAULT NOW(),
    position INT NOT NULL DEFAULT 0
);
CREATE INDEX idx_user_pins_user ON user_pins(user_id, position);
```

### API Endpoints
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/me/pins` | List user's pins |
| POST | `/api/me/pins` | Create pin |
| PUT | `/api/me/pins/:id` | Update pin |
| DELETE | `/api/me/pins/:id` | Delete pin |
| PUT | `/api/me/pins/reorder` | Reorder pins |

### Limits
- Max 50 pins per user

### UI
- Compact list view (icon + title/preview)
- Click note → expand inline to edit
- Click link → open in new tab
- Click message → navigate to channel
- "+" button with dropdown: Note / Link
- Drag to reorder within module (simpler than cross-module drag)

---

## Files Summary

### Backend
| File | Changes |
|------|---------|
| `server/migrations/` | Add `user_pins` table |
| `server/src/api/pins.rs` | CRUD handlers |
| `server/src/api/mod.rs` | Register routes |

### Frontend
| File | Changes |
|------|---------|
| `client/src/components/home/modules/CollapsibleModule.tsx` | Generic module wrapper |
| `client/src/components/home/modules/PendingModule.tsx` | Pending & suggestions |
| `client/src/components/home/modules/PinsModule.tsx` | Global pins |
| `client/src/components/home/modules/ActiveNowModule.tsx` | Refactored from HomeRightPanel |
| `client/src/components/home/HomeRightPanel.tsx` | Use module framework |
| `client/src/stores/pins.ts` | Pin state management |
| `client/src/stores/preferences.ts` | Add homeSidebar section |
| `client/src/lib/types.ts` | Pin types |

---

## Deferred (Future PRs)

- **Drag-drop module reordering** - Add when users request
- **Module visibility toggle** - Settings to hide/show modules
- **Server Pulse module** - Requires unread tracking infrastructure

---

## Testing

1. **Collapse/expand** - Toggle modules, verify state persists across refresh
2. **Pending module** - Accept/decline friend request, verify updates
3. **Pins CRUD** - Create, edit, delete notes and links
4. **Pin message** - Pin from channel context menu, verify appears in module
5. **Pin limits** - Verify 50 pin limit enforced
6. **Sync** - Change collapsed state on device A, verify syncs to device B
