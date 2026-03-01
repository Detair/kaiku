# Design: Diff-based State Sync (Phase 5)

## 1. Overview
Optimize bandwidth by replacing full object broadcasts with JSON patches for `User`, `Guild`, and `Member` updates.

## 2. Protocol Specification

### 2.1. New Event: `PatchEvent`
Instead of `UserUpdate { user: User }`, we introduce a generic patch event.

```json
{
  "op": "Patch",
  "d": {
    "entity_type": "user", // or "guild", "member"
    "entity_id": "uuid-string",
    "diff": {
      "avatar_url": "new_url.png",
      "status": "online"
    }
  }
}
```

## 3. Backend Implementation (Rust)

### 3.1. Diff Generation
We will use the `serde_json` `Value` to manually construct diffs for Phase 1 (safer than automated struct diffing initially).

In `server/src/social/handlers.rs` (update_profile):
```rust
// Old
// ws.broadcast(UserUpdate { user: updated_user });

// New
let mut diff = serde_json::Map::new();
if req.avatar_url.is_some() {
    diff.insert("avatar_url".into(), json!(req.avatar_url));
}
// ... check other fields

if !diff.is_empty() {
    ws.broadcast(PatchEvent {
        entity_type: "user",
        entity_id: user_id,
        diff: Value::Object(diff)
    });
}
```

### 3.2. Structs
Define `PatchEvent` in `server/src/ws/mod.rs` (or `events.rs`).

## 4. Frontend Implementation (Client)

### 4.1. Store Logic (`client/src/stores/websocket.ts`)
Update the main event switch:

```typescript
case "Patch":
  const { entity_type, entity_id, diff } = event.d;
  if (entity_type === 'user') {
    useUserStore.getState().patchUser(entity_id, diff);
  } else if (entity_type === 'guild') {
    useGuildStore.getState().patchGuild(entity_id, diff);
  }
  break;
```

### 4.2. User Store (`client/src/stores/users.ts`)
```typescript
patchUser: (id: string, diff: Partial<User>) => {
  set((state) => ({
    users: {
      ...state.users,
      [id]: { ...state.users[id], ...diff }
    }
  }));
}
```

## 5. Step-by-Step Plan
1.  **Server:** Define `PatchEvent` struct.
2.  **Server:** Refactor `update_user` handler to emit `PatchEvent` instead of `UserUpdate`.
3.  **Client:** Add `Patch` case to WebSocket handler.
4.  **Client:** Implement `patchUser` action in store.
5.  **Verify:** Monitor network traffic to confirm payload size reduction.