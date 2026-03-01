# Server-Synced User Preferences - Design

> **Status:** Approved
> **Date:** 2026-01-23

## Overview

Sync user preferences (theme, sound settings, quiet hours, per-channel notifications) across all devices in real-time.

---

## Requirements

- **Sync all preferences:** Theme, sound settings, quiet hours, connection display, per-channel notifications
- **Conflict resolution:** Last-write-wins using `updated_at` timestamp
- **Sync timing:** Push on change (debounced), pull on login
- **Real-time:** Broadcast changes to other devices via WebSocket

---

## Architecture

```
Login → Pull preferences from server → Merge with localStorage → Apply
        ↓
Settings change → Update localStorage → Debounce (500ms) → Push to server
        ↓
Server → Upsert DB → Broadcast via user:{user_id} channel
        ↓
Other devices → WebSocket event → Update localStorage → Apply
```

### Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Storage | JSONB column | Flexible schema, easy to extend |
| Sync channel | `user:{user_id}` | Reuse from PR #42 (read sync) |
| Conflict resolution | Last-write-wins | Simple, predictable |
| Push strategy | Debounced (500ms) | Prevent API spam on slider changes |

---

## Data Model

### Database Table

```sql
CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    preferences JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_user_preferences_updated ON user_preferences(updated_at);
```

### Preferences Schema

```typescript
interface UserPreferences {
  // Theme
  theme: "focused-hybrid" | "solarized-dark" | "solarized-light";

  // Sound settings
  sound: {
    enabled: boolean;
    volume: number;           // 0-100
    soundType: string;        // "default" | "subtle" | "ping" | "chime" | "bell"
    quietHours: {
      enabled: boolean;
      startTime: string;      // "HH:MM" format
      endTime: string;
    };
  };

  // Connection display
  connection: {
    displayMode: "circle" | "number";
    showNotifications: boolean;
  };

  // Per-channel notification levels
  channelNotifications: Record<string, "all" | "mentions" | "muted">;
}
```

---

## API Endpoints

### GET /api/me/preferences

Fetch current user's preferences.

**Response:**
```json
{
  "preferences": { ... },
  "updated_at": "2026-01-23T12:00:00Z"
}
```

**Behavior:**
- Returns empty object `{}` if no preferences saved
- Client merges with local defaults

### PUT /api/me/preferences

Update preferences (full replacement).

**Request:**
```json
{
  "preferences": { ... }
}
```

**Response:**
```json
{
  "preferences": { ... },
  "updated_at": "2026-01-23T12:00:01Z"
}
```

**Behavior:**
- Upserts `user_preferences` row
- Updates `updated_at` timestamp
- Broadcasts `PreferencesUpdated` event to `user:{user_id}` channel

### PATCH /api/me/preferences (Future)

Partial update for specific fields. Not in MVP scope.

---

## WebSocket Events

### ServerEvent::PreferencesUpdated

```rust
PreferencesUpdated {
    preferences: serde_json::Value,
    updated_at: DateTime<Utc>,
}
```

Sent to all user's sessions via `user:{user_id}` channel when preferences change.

**Client handling:**
1. Compare `updated_at` with local timestamp
2. If server is newer, update localStorage and apply
3. If local is newer, ignore (rare race condition)

---

## Client Implementation

### Sync Flow

```typescript
// On login
async function initPreferences(): Promise<void> {
  const server = await fetchPreferences();
  const local = loadFromLocalStorage();

  if (!server.preferences || Object.keys(server.preferences).length === 0) {
    // No server prefs, push local
    await pushPreferences(local);
  } else if (server.updated_at > local.updated_at) {
    // Server is newer, apply
    applyPreferences(server.preferences);
    saveToLocalStorage(server.preferences, server.updated_at);
  } else {
    // Local is newer (edited while offline), push
    await pushPreferences(local);
  }
}

// On settings change
const debouncedPush = debounce(async (prefs: UserPreferences) => {
  await pushPreferences(prefs);
}, 500);

function onSettingChange(key: string, value: any): void {
  const updated = updateLocalStorage(key, value);
  applyPreferences(updated);
  debouncedPush(updated);
}

// On WebSocket event
function handlePreferencesUpdated(event: PreferencesUpdatedEvent): void {
  const local = loadFromLocalStorage();
  if (event.updated_at > local.updated_at) {
    applyPreferences(event.preferences);
    saveToLocalStorage(event.preferences, event.updated_at);
  }
}
```

### localStorage Structure

```typescript
// Key: "vc:preferences"
interface StoredPreferences {
  data: UserPreferences;
  updated_at: string;  // ISO timestamp
}
```

---

## Migration Strategy

### Existing localStorage Data

On first sync-enabled login:
1. Read existing localStorage keys (`theme`, `vc:soundSettings`, etc.)
2. Merge into unified `UserPreferences` object
3. Push to server
4. Clear old localStorage keys
5. Use new unified key going forward

### Backward Compatibility

- Old clients without sync continue using localStorage
- Server preferences are additive, don't break old clients

---

## Files Summary

### Backend

| File | Changes |
|------|---------|
| `server/migrations/` | Add `user_preferences` table |
| `server/src/api/preferences.rs` | New module with GET/PUT handlers |
| `server/src/api/mod.rs` | Register routes |
| `server/src/ws/mod.rs` | Add `PreferencesUpdated` event variant |

### Frontend

| File | Changes |
|------|---------|
| `client/src/stores/preferences.ts` | New unified preferences store with sync |
| `client/src/stores/theme.ts` | Migrate to use preferences store |
| `client/src/stores/sound.ts` | Migrate to use preferences store |
| `client/src/stores/connection.ts` | Migrate to use preferences store |
| `client/src/stores/websocket.ts` | Handle `preferences_updated` event |
| `client/src/lib/types.ts` | Add `PreferencesUpdated` event type |

---

## Testing

1. **Single device:** Change settings, refresh, verify persisted
2. **Two devices:** Change on A, verify B updates within 2 seconds
3. **Offline:** Change while offline, reconnect, verify syncs
4. **Conflict:** Change on both devices quickly, verify last-write-wins
5. **Migration:** Existing user with localStorage, verify migrates on login

---

## Future Considerations

- **PATCH endpoint:** Partial updates for bandwidth efficiency
- **Sync status indicator:** Show "syncing..." in UI
- **Conflict resolution UI:** Let user choose on conflict (if needed)
- **Export/Import:** Allow users to export preferences as JSON
