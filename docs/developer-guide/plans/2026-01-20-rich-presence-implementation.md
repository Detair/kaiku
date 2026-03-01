# Rich Presence Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Display "Playing X" game activity status for users in Member Lists and User Popups

**Architecture:** Tauri polls running processes via `sysinfo`, matches against `games.json`, sends activity to server via WebSocket, server broadcasts to subscribers via Redis pub/sub

**Tech Stack:** Rust (sysinfo, serde_json), TypeScript/Solid.js, PostgreSQL (JSONB), Redis pub/sub

---

## Task 1: Database Migration - Add Activity Column

**Files:**
- Create: `server/migrations/20260120000000_add_user_activity.sql`

**Step 1: Write the migration**

```sql
-- Add activity column for rich presence data
ALTER TABLE users ADD COLUMN activity JSONB;

-- Index for efficient NULL checks (users with active activity)
CREATE INDEX idx_users_activity_not_null ON users ((activity IS NOT NULL)) WHERE activity IS NOT NULL;

COMMENT ON COLUMN users.activity IS 'Rich presence activity data (game, music, etc). NULL = no activity.';
```

**Step 2: Run migration**

Run: `cd server && sqlx migrate run`
Expected: Migration applied successfully

**Step 3: Update sqlx offline cache**

Run: `cd server && cargo sqlx prepare`
Expected: Query cache updated

**Step 4: Commit**

```bash
git add server/migrations/
git commit -m "feat(db): add activity column for rich presence"
```

---

## Task 2: Server Types - Activity Structs

**Files:**
- Create: `server/src/presence/mod.rs`
- Create: `server/src/presence/types.rs`
- Modify: `server/src/lib.rs` - add module

**Step 1: Write the failing test**

```rust
// In server/src/presence/types.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_serialization() {
        let activity = Activity {
            activity_type: ActivityType::Game,
            name: "Minecraft".to_string(),
            started_at: chrono::Utc::now(),
            details: None,
        };
        let json = serde_json::to_string(&activity).unwrap();
        assert!(json.contains("Minecraft"));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd server && cargo test test_activity_serialization`
Expected: FAIL with "module not found"

**Step 3: Write the types**

```rust
// server/src/presence/types.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Type of activity the user is engaged in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Game,
    Listening,
    Watching,
    Coding,
    Custom,
}

/// Rich presence activity data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    /// Type of activity.
    #[serde(rename = "type")]
    pub activity_type: ActivityType,
    /// Display name (e.g., "Minecraft", "VS Code").
    pub name: String,
    /// When the activity started.
    pub started_at: DateTime<Utc>,
    /// Optional details (e.g., "Creative Mode", "Editing main.rs").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

// server/src/presence/mod.rs
mod types;
pub use types::*;
```

**Step 4: Register module in lib.rs**

Add to `server/src/lib.rs`:
```rust
pub mod presence;
```

**Step 5: Run test to verify it passes**

Run: `cd server && cargo test test_activity_serialization`
Expected: PASS

**Step 6: Commit**

```bash
git add server/src/presence/ server/src/lib.rs
git commit -m "feat(presence): add Activity types for rich presence"
```

---

## Task 3: Server WebSocket - Activity Events

**Files:**
- Modify: `server/src/ws/mod.rs` - add SetActivity client event and RichPresenceUpdate server event

**Step 1: Add ClientEvent variant**

In `server/src/ws/mod.rs`, add to `ClientEvent` enum:
```rust
/// Set rich presence activity (game, music, etc).
SetActivity {
    activity: Option<crate::presence::Activity>,
},
```

**Step 2: Add ServerEvent variant**

Add to `ServerEvent` enum:
```rust
/// Rich presence activity update.
RichPresenceUpdate {
    user_id: Uuid,
    activity: Option<crate::presence::Activity>,
},
```

**Step 3: Add handler for SetActivity**

In `handle_client_message` function, add match arm:
```rust
ClientEvent::SetActivity { activity } => {
    // Update database
    sqlx::query("UPDATE users SET activity = $1 WHERE id = $2")
        .bind(serde_json::to_value(&activity).ok())
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to update activity: {}", e))?;

    // Broadcast to user's presence subscribers
    let event = ServerEvent::RichPresenceUpdate {
        user_id,
        activity,
    };
    broadcast_presence_update(&state, user_id, &event).await;
    Ok(())
}
```

**Step 4: Implement broadcast_presence_update function**

```rust
/// Broadcast a presence update to all users who should see it (friends, guild members).
async fn broadcast_presence_update(state: &AppState, user_id: Uuid, event: &ServerEvent) {
    let json = match serde_json::to_string(event) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!("Failed to serialize presence event: {}", e);
            return;
        }
    };

    // Broadcast on presence channel
    let channel = format!("presence:{}", user_id);
    if let Err(e) = state.redis.publish::<(), _, _>(&channel, &json).await {
        tracing::error!("Failed to broadcast presence update: {}", e);
    }
}
```

**Step 5: Run cargo check**

Run: `cd server && cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add SetActivity and RichPresenceUpdate events"
```

---

## Task 4: Server - Subscribe to Presence Updates

**Files:**
- Modify: `server/src/ws/mod.rs` - subscribe to friends' presence on connect

**Step 1: Add presence subscription on connect**

In `handle_socket` after user connects, subscribe to friends' presence channels:
```rust
// Subscribe to friends' presence updates
let friends = get_user_friends(&state, user_id).await.unwrap_or_default();
for friend_id in &friends {
    let channel = format!("presence:{}", friend_id);
    if let Err(e) = state.redis.subscribe::<(), _>(&channel).await {
        tracing::warn!("Failed to subscribe to presence for {}: {}", friend_id, e);
    }
}
```

**Step 2: Add get_user_friends helper**

```rust
async fn get_user_friends(state: &AppState, user_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    let friends: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT CASE
            WHEN user1_id = $1 THEN user2_id
            ELSE user1_id
        END as friend_id
        FROM friendships
        WHERE (user1_id = $1 OR user2_id = $1)
        AND status = 'accepted'
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(friends.into_iter().map(|(id,)| id).collect())
}
```

**Step 3: Run cargo check**

Run: `cd server && cargo check`
Expected: No errors

**Step 4: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): subscribe to friends' presence updates on connect"
```

---

## Task 5: Tauri - Games Database

**Files:**
- Create: `client/src-tauri/src/presence/mod.rs`
- Create: `client/src-tauri/src/presence/games.rs`
- Create: `client/src-tauri/resources/games.json`

**Step 1: Create games.json database**

```json
{
  "games": [
    {
      "process_names": ["minecraft.exe", "javaw.exe"],
      "match_args": ["minecraft"],
      "name": "Minecraft",
      "type": "game"
    },
    {
      "process_names": ["code.exe", "code"],
      "name": "Visual Studio Code",
      "type": "coding"
    },
    {
      "process_names": ["discord.exe", "discord"],
      "name": "Discord",
      "type": "custom"
    },
    {
      "process_names": ["steam.exe"],
      "name": "Steam",
      "type": "custom"
    },
    {
      "process_names": ["spotify.exe", "spotify"],
      "name": "Spotify",
      "type": "listening"
    },
    {
      "process_names": ["vlc.exe", "vlc"],
      "name": "VLC Media Player",
      "type": "watching"
    },
    {
      "process_names": ["firefox.exe", "firefox"],
      "name": "Firefox",
      "type": "custom"
    },
    {
      "process_names": ["chrome.exe", "chrome"],
      "name": "Google Chrome",
      "type": "custom"
    },
    {
      "process_names": ["leagueclient.exe", "league of legends.exe"],
      "name": "League of Legends",
      "type": "game"
    },
    {
      "process_names": ["valorant.exe", "valorant-win64-shipping.exe"],
      "name": "Valorant",
      "type": "game"
    },
    {
      "process_names": ["csgo.exe", "cs2.exe"],
      "name": "Counter-Strike",
      "type": "game"
    },
    {
      "process_names": ["fortnite.exe", "fortniteclient-win64-shipping.exe"],
      "name": "Fortnite",
      "type": "game"
    },
    {
      "process_names": ["rocketleague.exe"],
      "name": "Rocket League",
      "type": "game"
    },
    {
      "process_names": ["overwatch.exe"],
      "name": "Overwatch",
      "type": "game"
    },
    {
      "process_names": ["apex_legends.exe", "r5apex.exe"],
      "name": "Apex Legends",
      "type": "game"
    }
  ]
}
```

**Step 2: Create games.rs module**

```rust
// client/src-tauri/src/presence/games.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub process_names: Vec<String>,
    #[serde(default)]
    pub match_args: Vec<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub activity_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamesDatabase {
    pub games: Vec<GameEntry>,
}

impl GamesDatabase {
    pub fn load() -> Self {
        let json = include_str!("../../resources/games.json");
        serde_json::from_str(json).expect("Invalid games.json")
    }

    pub fn find_by_process(&self, process_name: &str) -> Option<&GameEntry> {
        let lower = process_name.to_lowercase();
        self.games.iter().find(|g| {
            g.process_names.iter().any(|p| p.to_lowercase() == lower)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_games_database() {
        let db = GamesDatabase::load();
        assert!(!db.games.is_empty());
    }

    #[test]
    fn test_find_minecraft() {
        let db = GamesDatabase::load();
        let game = db.find_by_process("minecraft.exe");
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Minecraft");
    }
}
```

**Step 3: Create mod.rs**

```rust
// client/src-tauri/src/presence/mod.rs
mod games;
pub use games::*;
```

**Step 4: Run tests**

Run: `cd client/src-tauri && cargo test test_load_games test_find_minecraft`
Expected: PASS

**Step 5: Commit**

```bash
git add client/src-tauri/src/presence/ client/src-tauri/resources/
git commit -m "feat(tauri): add games database for process matching"
```

---

## Task 6: Tauri - Process Scanner

**Files:**
- Modify: `client/src-tauri/Cargo.toml` - add sysinfo dependency
- Create: `client/src-tauri/src/presence/scanner.rs`
- Modify: `client/src-tauri/src/presence/mod.rs`

**Step 1: Add sysinfo dependency**

In `client/src-tauri/Cargo.toml`:
```toml
[dependencies]
sysinfo = "0.30"
```

**Step 2: Create scanner.rs**

```rust
// client/src-tauri/src/presence/scanner.rs
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use super::{GameEntry, GamesDatabase};

pub struct ProcessScanner {
    system: System,
    games_db: GamesDatabase,
}

impl ProcessScanner {
    pub fn new() -> Self {
        Self {
            system: System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new())
            ),
            games_db: GamesDatabase::load(),
        }
    }

    /// Refresh process list and find matching game.
    pub fn scan(&mut self) -> Option<GameEntry> {
        self.system.refresh_processes();

        for (_pid, process) in self.system.processes() {
            let name = process.name().to_string_lossy();
            if let Some(game) = self.games_db.find_by_process(&name) {
                return Some(game.clone());
            }
        }
        None
    }
}

impl Default for ProcessScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_creation() {
        let scanner = ProcessScanner::new();
        assert!(!scanner.games_db.games.is_empty());
    }
}
```

**Step 3: Update mod.rs**

```rust
// client/src-tauri/src/presence/mod.rs
mod games;
mod scanner;

pub use games::*;
pub use scanner::*;
```

**Step 4: Run tests**

Run: `cd client/src-tauri && cargo test test_scanner`
Expected: PASS

**Step 5: Commit**

```bash
git add client/src-tauri/Cargo.toml client/src-tauri/src/presence/
git commit -m "feat(tauri): add process scanner using sysinfo"
```

---

## Task 7: Tauri - Presence Commands

**Files:**
- Create: `client/src-tauri/src/commands/presence.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`
- Modify: `client/src-tauri/src/lib.rs`

**Step 1: Create presence commands**

```rust
// client/src-tauri/src/commands/presence.rs
use serde::{Deserialize, Serialize};
use tauri::command;
use std::sync::Mutex;
use crate::presence::{GameEntry, ProcessScanner};

/// Detected activity from process scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedActivity {
    pub name: String,
    #[serde(rename = "type")]
    pub activity_type: String,
}

/// Global scanner state.
static SCANNER: std::sync::OnceLock<Mutex<ProcessScanner>> = std::sync::OnceLock::new();

fn get_scanner() -> &'static Mutex<ProcessScanner> {
    SCANNER.get_or_init(|| Mutex::new(ProcessScanner::new()))
}

/// Scan running processes for known games/applications.
#[command]
pub fn scan_processes() -> Option<DetectedActivity> {
    let mut scanner = get_scanner().lock().ok()?;
    scanner.scan().map(|game| DetectedActivity {
        name: game.name,
        activity_type: game.activity_type,
    })
}

/// Get list of all known games for settings UI.
#[command]
pub fn get_known_games() -> Vec<String> {
    let scanner = get_scanner().lock().ok();
    scanner
        .map(|s| s.games_db.games.iter().map(|g| g.name.clone()).collect())
        .unwrap_or_default()
}
```

**Step 2: Register in commands/mod.rs**

Add:
```rust
pub mod presence;
```

**Step 3: Register commands in lib.rs**

Add to invoke_handler:
```rust
commands::presence::scan_processes,
commands::presence::get_known_games,
```

**Step 4: Run cargo check**

Run: `cd client/src-tauri && cargo check`
Expected: No errors

**Step 5: Commit**

```bash
git add client/src-tauri/src/commands/
git commit -m "feat(tauri): add presence commands for process scanning"
```

---

## Task 8: Tauri - Background Presence Polling

**Files:**
- Create: `client/src-tauri/src/presence/service.rs`
- Modify: `client/src-tauri/src/presence/mod.rs`
- Modify: `client/src-tauri/src/lib.rs` - start service on app ready

**Step 1: Create presence service**

```rust
// client/src-tauri/src/presence/service.rs
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::time::interval;

use super::ProcessScanner;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Start background presence polling.
pub fn start_presence_service(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return; // Already running
    }

    tauri::async_runtime::spawn(async move {
        let mut scanner = ProcessScanner::new();
        let mut last_activity: Option<String> = None;
        let mut ticker = interval(Duration::from_secs(15));

        loop {
            ticker.tick().await;

            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }

            let current = scanner.scan().map(|g| g.name.clone());

            // Only emit if activity changed
            if current != last_activity {
                let payload = current.as_ref().map(|name| {
                    serde_json::json!({
                        "name": name,
                        "type": "game",
                        "started_at": chrono::Utc::now().to_rfc3339()
                    })
                });

                let _ = app.emit("presence:activity_changed", payload);
                last_activity = current;
            }
        }
    });
}

/// Stop background presence polling.
pub fn stop_presence_service() {
    RUNNING.store(false, Ordering::SeqCst);
}
```

**Step 2: Update mod.rs**

```rust
// client/src-tauri/src/presence/mod.rs
mod games;
mod scanner;
mod service;

pub use games::*;
pub use scanner::*;
pub use service::*;
```

**Step 3: Start service in lib.rs**

In the setup function or on_ready hook:
```rust
// After app is ready
presence::start_presence_service(app.handle().clone());
```

**Step 4: Run cargo check**

Run: `cd client/src-tauri && cargo check`
Expected: No errors

**Step 5: Commit**

```bash
git add client/src-tauri/src/presence/ client/src-tauri/src/lib.rs
git commit -m "feat(tauri): add background presence polling service"
```

---

## Task 9: Client Types - Activity Interface

**Files:**
- Modify: `client/src/lib/types.ts`

**Step 1: Add Activity types**

```typescript
// In client/src/lib/types.ts

export type ActivityType = "game" | "listening" | "watching" | "coding" | "custom";

export interface Activity {
  type: ActivityType;
  name: string;
  started_at: string;
  details?: string;
}

export interface UserPresence {
  status: UserStatus;
  activity?: Activity | null;
  lastSeen?: string;
}
```

**Step 2: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(types): add Activity interface for rich presence"
```

---

## Task 10: Client Store - Presence with Activity

**Files:**
- Modify: `client/src/stores/presence.ts`

**Step 1: Update presence store**

```typescript
// Update UserPresence interface to include activity
interface UserPresence {
  status: UserStatus;
  activity?: Activity | null;
  lastSeen?: string;
}

// Update updateUserPresence function
export function updateUserPresence(
  userId: string,
  status: UserStatus,
  activity?: Activity | null
): void {
  setPresenceState(
    produce((state) => {
      state.users[userId] = {
        status,
        activity: activity ?? state.users[userId]?.activity,
        lastSeen: status === "offline" ? new Date().toISOString() : undefined,
      };
    })
  );
}

// Add new function for activity-only updates
export function updateUserActivity(userId: string, activity: Activity | null): void {
  setPresenceState(
    produce((state) => {
      if (state.users[userId]) {
        state.users[userId].activity = activity;
      } else {
        state.users[userId] = {
          status: "online",
          activity,
        };
      }
    })
  );
}

// Add getter for user activity
export function getUserActivity(userId: string): Activity | null | undefined {
  return presenceState.users[userId]?.activity;
}
```

**Step 2: Add Tauri event listener for activity changes**

```typescript
// In initPresence function, add:
if (isTauri) {
  const { listen } = await import("@tauri-apps/api/event");

  // Listen for local activity changes
  await listen<Activity | null>("presence:activity_changed", (event) => {
    // Send to server via WebSocket
    const ws = getWebSocketManager();
    if (ws) {
      ws.send({ type: "set_activity", activity: event.payload });
    }
  });

  // Listen for remote activity updates
  await listen<{ user_id: string; activity: Activity | null }>(
    "ws:rich_presence_update",
    (event) => {
      updateUserActivity(event.payload.user_id, event.payload.activity);
    }
  );
}
```

**Step 3: Commit**

```bash
git add client/src/stores/presence.ts
git commit -m "feat(store): extend presence store with activity support"
```

---

## Task 11: Client WebSocket - Handle Activity Events

**Files:**
- Modify: `client/src-tauri/src/commands/websocket.rs` - handle RichPresenceUpdate
- Modify: `client/src/stores/websocket.ts` - connect event to store

**Step 1: Update Tauri websocket handler**

In `handle_server_event` function, add case for RichPresenceUpdate:
```rust
"rich_presence_update" => {
    if let (Some(user_id), activity) = (
        event.get("user_id").and_then(|v| v.as_str()),
        event.get("activity").cloned(),
    ) {
        let _ = app.emit("ws:rich_presence_update", serde_json::json!({
            "user_id": user_id,
            "activity": activity
        }));
    }
}
```

**Step 2: Update websocket.ts store**

Add to listeners in initWebSocket:
```typescript
unlisteners.push(
  await listen<{ user_id: string; activity: Activity | null }>(
    "ws:rich_presence_update",
    (event) => {
      updateUserActivity(event.payload.user_id, event.payload.activity);
    }
  )
);
```

**Step 3: Commit**

```bash
git add client/src-tauri/src/commands/websocket.rs client/src/stores/websocket.ts
git commit -m "feat(ws): handle RichPresenceUpdate events"
```

---

## Task 12: UI - Activity Indicator Component

**Files:**
- Create: `client/src/components/ui/ActivityIndicator.tsx`

**Step 1: Create the component**

```tsx
// client/src/components/ui/ActivityIndicator.tsx
import { Component, Show } from "solid-js";
import { Gamepad2, Music, Monitor, Code, Sparkles } from "lucide-solid";
import type { Activity, ActivityType } from "../../lib/types";

interface ActivityIndicatorProps {
  activity: Activity;
  compact?: boolean;
}

const activityIcons: Record<ActivityType, typeof Gamepad2> = {
  game: Gamepad2,
  listening: Music,
  watching: Monitor,
  coding: Code,
  custom: Sparkles,
};

const activityLabels: Record<ActivityType, string> = {
  game: "Playing",
  listening: "Listening to",
  watching: "Watching",
  coding: "Coding in",
  custom: "Using",
};

const ActivityIndicator: Component<ActivityIndicatorProps> = (props) => {
  const Icon = () => activityIcons[props.activity.type] || Sparkles;
  const label = () => activityLabels[props.activity.type] || "Using";

  return (
    <div class="flex items-center gap-1.5 text-xs text-purple-400">
      <Icon class="w-3 h-3" />
      <Show when={!props.compact}>
        <span class="text-text-tertiary">{label()}</span>
      </Show>
      <span class="font-medium truncate max-w-[120px]">{props.activity.name}</span>
    </div>
  );
};

export default ActivityIndicator;
```

**Step 2: Export from index**

Add to `client/src/components/ui/index.ts`:
```typescript
export { default as ActivityIndicator } from "./ActivityIndicator";
```

**Step 3: Commit**

```bash
git add client/src/components/ui/ActivityIndicator.tsx client/src/components/ui/index.ts
git commit -m "feat(ui): add ActivityIndicator component"
```

---

## Task 13: UI - Integrate into MembersTab

**Files:**
- Modify: `client/src/components/guilds/MembersTab.tsx`

**Step 1: Import and use ActivityIndicator**

Add imports:
```typescript
import { getUserActivity } from "../../stores/presence";
import ActivityIndicator from "../ui/ActivityIndicator";
```

**Step 2: Add activity display below username**

In the member list item, after the username/status line:
```tsx
{/* Activity indicator */}
<Show when={getUserActivity(member.user_id)}>
  <ActivityIndicator
    activity={getUserActivity(member.user_id)!}
    compact
  />
</Show>
```

**Step 3: Commit**

```bash
git add client/src/components/guilds/MembersTab.tsx
git commit -m "feat(ui): show activity in MembersTab"
```

---

## Task 14: UI - Integrate into User Popover

**Files:**
- Modify: `client/src/components/ui/UserPopover.tsx` (or create if doesn't exist)

**Step 1: Find or create UserPopover component**

If exists, add activity section. If not, create minimal version:

```tsx
// In UserPopover, add after status display:
<Show when={getUserActivity(props.userId)}>
  <div class="px-4 py-3 border-t border-white/10">
    <div class="text-xs text-text-tertiary uppercase mb-2">Activity</div>
    <ActivityIndicator activity={getUserActivity(props.userId)!} />
    <Show when={getUserActivity(props.userId)?.started_at}>
      <div class="text-xs text-text-tertiary mt-1">
        Started {formatRelativeTime(getUserActivity(props.userId)!.started_at)}
      </div>
    </Show>
  </div>
</Show>
```

**Step 2: Commit**

```bash
git add client/src/components/ui/
git commit -m "feat(ui): show activity in user popover"
```

---

## Task 15: Privacy Settings

**Files:**
- Modify: `client/src/components/settings/PrivacySettings.tsx` (create if needed)
- Modify: `client/src-tauri/src/presence/service.rs` - respect privacy setting

**Step 1: Add privacy toggle**

Create or update PrivacySettings:
```tsx
// Toggle for "Share game activity"
<div class="flex items-center justify-between">
  <div>
    <div class="font-medium text-text-primary">Display current activity</div>
    <div class="text-sm text-text-secondary">
      Show what game you're playing to friends
    </div>
  </div>
  <Switch
    checked={shareActivity()}
    onChange={setShareActivity}
  />
</div>
```

**Step 2: Store setting in localStorage**

```typescript
const [shareActivity, setShareActivity] = createSignal(
  localStorage.getItem("privacy.shareActivity") !== "false"
);

createEffect(() => {
  localStorage.setItem("privacy.shareActivity", String(shareActivity()));
});
```

**Step 3: Commit**

```bash
git add client/src/components/settings/
git commit -m "feat(settings): add activity privacy toggle"
```

---

## Task 16: Integration Test

**Files:**
- Create: `client/src-tauri/tests/presence_test.rs`

**Step 1: Write integration test**

```rust
// client/src-tauri/tests/presence_test.rs

#[test]
fn test_games_database_loads() {
    let db = canis_client::presence::GamesDatabase::load();
    assert!(!db.games.is_empty(), "Games database should not be empty");
}

#[test]
fn test_process_scanner_initializes() {
    let scanner = canis_client::presence::ProcessScanner::new();
    // Just verify it doesn't panic
}

#[test]
fn test_known_games_list() {
    let db = canis_client::presence::GamesDatabase::load();
    let names: Vec<_> = db.games.iter().map(|g| &g.name).collect();
    assert!(names.contains(&&"Minecraft".to_string()));
    assert!(names.contains(&&"Visual Studio Code".to_string()));
}
```

**Step 2: Run tests**

Run: `cd client/src-tauri && cargo test presence`
Expected: All tests pass

**Step 3: Commit**

```bash
git add client/src-tauri/tests/
git commit -m "test(tauri): add presence integration tests"
```

---

## Task 17: Server Tests

**Files:**
- Create: `server/tests/presence_test.rs`

**Step 1: Write presence type tests**

```rust
// server/tests/presence_test.rs

use canis_server::presence::{Activity, ActivityType};
use chrono::Utc;

#[test]
fn test_activity_serialization() {
    let activity = Activity {
        activity_type: ActivityType::Game,
        name: "Minecraft".to_string(),
        started_at: Utc::now(),
        details: Some("Survival Mode".to_string()),
    };

    let json = serde_json::to_string(&activity).unwrap();
    assert!(json.contains("\"type\":\"game\""));
    assert!(json.contains("\"name\":\"Minecraft\""));
    assert!(json.contains("\"details\":\"Survival Mode\""));
}

#[test]
fn test_activity_deserialization() {
    let json = r#"{
        "type": "game",
        "name": "Valorant",
        "started_at": "2026-01-20T12:00:00Z"
    }"#;

    let activity: Activity = serde_json::from_str(json).unwrap();
    assert_eq!(activity.activity_type, ActivityType::Game);
    assert_eq!(activity.name, "Valorant");
    assert!(activity.details.is_none());
}

#[test]
fn test_activity_type_variants() {
    assert_eq!(
        serde_json::to_string(&ActivityType::Game).unwrap(),
        "\"game\""
    );
    assert_eq!(
        serde_json::to_string(&ActivityType::Listening).unwrap(),
        "\"listening\""
    );
    assert_eq!(
        serde_json::to_string(&ActivityType::Coding).unwrap(),
        "\"coding\""
    );
}
```

**Step 2: Run tests**

Run: `cd server && cargo test presence`
Expected: All tests pass

**Step 3: Commit**

```bash
git add server/tests/presence_test.rs
git commit -m "test(server): add presence type tests"
```

---

## Task 18: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add Rich Presence entry**

Under `[Unreleased]` -> `### Added`:
```markdown
- Rich Presence (Game Activity) showing "Playing X" status in member lists
  - Automatic game detection via process scanning (sysinfo)
  - 15+ pre-configured games (Minecraft, Valorant, League of Legends, etc.)
  - Activity display in member lists and user popovers
  - Privacy toggle in settings to disable activity sharing
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add Rich Presence to CHANGELOG"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Database migration | 1 migration |
| 2 | Server Activity types | 2 new files |
| 3 | WebSocket events | 1 modified |
| 4 | Presence subscription | 1 modified |
| 5 | Games database | 3 new files |
| 6 | Process scanner | 2 files |
| 7 | Tauri commands | 2 files |
| 8 | Background polling | 2 files |
| 9 | Client types | 1 modified |
| 10 | Presence store | 1 modified |
| 11 | WebSocket handler | 2 files |
| 12 | ActivityIndicator | 2 files |
| 13 | MembersTab integration | 1 modified |
| 14 | UserPopover integration | 1 file |
| 15 | Privacy settings | 2 files |
| 16 | Tauri tests | 1 new file |
| 17 | Server tests | 1 new file |
| 18 | CHANGELOG | 1 modified |

**Total: 18 tasks, ~25 files**
