# Guild Management Modal Design

## Overview

A modal dialog for guild owners and members to manage invites and view/manage members. Accessible via gear icon in the sidebar guild header.

## Scope (MVP)

- **Invites Tab** (owner only): Create and manage shareable invite links
- **Members Tab** (all members): View members, search, kick (owner only)

## Modal Structure

```
┌─────────────────────────────────────────────┐
│  [Guild Icon] Guild Name           [X Close] │
├─────────────────────────────────────────────┤
│  [Invites]  [Members]                        │  ← Tab bar
├─────────────────────────────────────────────┤
│                                              │
│  Tab content area                            │
│                                              │
└─────────────────────────────────────────────┘
```

**Access Control:**
- All guild members can view the Members tab
- Only the guild owner can access the Invites tab and kick members
- Non-owners see a simplified modal (Members tab only, no actions)

**Opening the Modal:**
- Gear icon next to guild name in sidebar header
- Tooltip: "Guild Settings"

## Invites Tab

### Layout

```
┌─────────────────────────────────────────────────────┐
│ Create New Invite                                    │
│ ┌─────────────────────────────────────────────────┐ │
│ │ Expires after: [7 days ▼]       [Create Invite] │ │
│ └─────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────┤
│ Active Invites                                       │
│ ┌─────────────────────────────────────────────────┐ │
│ │ https://server.com/invite/Xk9mP2qL   [Copy] [🗑] │ │
│ │ Expires in 6 days • 3 uses                       │ │
│ ├─────────────────────────────────────────────────┤ │
│ │ https://server.com/invite/Ab3nR7wZ   [Copy] [🗑] │ │
│ │ Expires in 2 hours • 0 uses                      │ │
│ └─────────────────────────────────────────────────┘ │
│                                                      │
│ ── Empty state: "No active invites" ──              │
└─────────────────────────────────────────────────────┘
```

### Expiry Options (dropdown)

- 30 minutes
- 1 hour
- 1 day
- 7 days (default)
- Never

### Invite Code Specification

- **Length:** 8 characters
- **Alphabet:** A-Z, a-z, 0-9 (62 chars)
- **Generation:** Cryptographically random
- **Uniqueness:** Globally unique (simple join flow)
- **Combinations:** 62^8 = 218 trillion

### Each Invite Row Shows

- Full copyable URL (not just code)
- Time remaining until expiry
- Usage count (how many joined)
- Delete button with confirmation

### Rate Limiting

- Max 10 invites per guild per hour

## Members Tab

### Layout

```
┌─────────────────────────────────────────────────────┐
│ 🔍 Search members...                                 │
├─────────────────────────────────────────────────────┤
│ 12 Members                                           │
│ ┌─────────────────────────────────────────────────┐ │
│ │ [Avatar] detair              👑 Owner            │ │
│ │          @detair                                 │ │
│ │          Joined Jan 14 • Online                🟢│ │
│ ├─────────────────────────────────────────────────┤ │
│ │ [Avatar] alice                        [Kick ✕]  │ │
│ │          @alice_wonder                          │ │
│ │          Joined Jan 14 • 2 hours ago          ⚫│ │
│ └─────────────────────────────────────────────────┘ │
│                                                      │
│        [Load more...]  (if 50+ members)             │
└─────────────────────────────────────────────────────┘
```

### Search

- Filters by display name OR username
- Client-side filtering for <100 members
- Debounced input (300ms)

### Member Row Shows

- Avatar (initials fallback)
- Display name + crown icon if owner
- Username with @ prefix
- Join date
- Last seen time + status indicator

### Last Online Display

| Condition | Display |
|-----------|---------|
| Online now | "Online" 🟢 |
| < 1 hour | "X minutes ago" ⚫ |
| < 24 hours | "X hours ago" ⚫ |
| < 7 days | "X days ago" ⚫ |
| Older | "Jan 10, 2026" ⚫ |
| Never | "Never" ⚫ |

### Status Indicators

- 🟢 Green = Online now
- 🟡 Yellow = Idle (online but inactive 5+ min)
- ⚫ Gray = Offline

### Kick Flow (inline confirmation)

```
Click [Kick] → Button changes to [Confirm?] (red)
             → Click again within 3s to confirm
             → Or click away to cancel
```

### Access Control

- Owner sees kick buttons on everyone except themselves
- Non-owners see no kick buttons
- Owner cannot kick themselves (button not rendered)

### Pagination

- Load 50 members initially
- "Load more" button fetches next 50
- API: `GET /api/guilds/:id/members?limit=50&offset=0`

### Empty States

- No members besides owner: "You're the only one here. Invite some friends!"
- Search with no results: "No members match your search"

## Database Schema

### New Table: guild_invites

```sql
CREATE TABLE guild_invites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    code VARCHAR(8) NOT NULL UNIQUE,
    created_by UUID NOT NULL REFERENCES users(id),
    expires_at TIMESTAMPTZ,
    use_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_guild_invites_code ON guild_invites(code);
CREATE INDEX idx_guild_invites_guild ON guild_invites(guild_id);
```

### Users Table Addition

```sql
ALTER TABLE users ADD COLUMN last_seen_at TIMESTAMPTZ;
CREATE INDEX idx_users_last_seen ON users(last_seen_at DESC);
```

### Updating last_seen_at

- Set on WebSocket connect/activity
- Set on API request (via auth middleware)
- Updated every ~60 seconds while active (not every request)

## API Endpoints

| Method | Endpoint | Description | Auth |
|--------|----------|-------------|------|
| GET | `/api/guilds/:id/invites` | List active invites | Owner only |
| POST | `/api/guilds/:id/invites` | Create invite | Owner only |
| DELETE | `/api/guilds/:id/invites/:code` | Revoke invite | Owner only |
| POST | `/api/invites/:code/join` | Join guild via invite | Any user |
| GET | `/api/guilds/:id/members` | List members (paginated) | Members |
| DELETE | `/api/guilds/:id/members/:userId` | Kick member | Owner only |

### Rate Limits

- Create invite: 10/hour per guild
- Join via invite: 5/minute per user

## New Files

| File | Purpose |
|------|---------|
| `migrations/XXXXXX_guild_invites.sql` | Invites table + last_seen column |
| `server/src/guild/invites.rs` | Invite handlers |
| `client/src/components/guilds/GuildSettingsModal.tsx` | Main modal |
| `client/src/components/guilds/InvitesTab.tsx` | Invites management |
| `client/src/components/guilds/MembersTab.tsx` | Members list |
| `client/src/stores/guild.ts` | Guild state (invites, members) |
| `client/src/lib/tauri.ts` | Add invite/member API calls |

## Type Definitions

### GuildMember (updated)

```typescript
export interface GuildMember {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  nickname: string | null;
  joined_at: string;
  status: "online" | "idle" | "offline";
  last_seen_at: string | null;
}
```

### GuildInvite (new)

```typescript
export interface GuildInvite {
  id: string;
  guild_id: string;
  code: string;
  created_by: string;
  expires_at: string | null;
  use_count: number;
  created_at: string;
}
```

### CreateInviteRequest

```typescript
export interface CreateInviteRequest {
  expires_in: "30m" | "1h" | "1d" | "7d" | "never";
}
```

## Persona Review Summary

| Persona | Feedback | Status |
|---------|----------|--------|
| Elrond | Globally unique codes, proper indexing | Incorporated |
| Éowyn | Visible gear icon for discoverability | Incorporated |
| Faramir | Crypto-random codes, rate limiting, kick confirmation | Incorporated |
| Legolas | Empty states, owner self-kick prevention | Incorporated |
| Pippin | Copy full URL button, usage count | Incorporated |
| Gandalf | Pagination-ready API, user search | Incorporated |
