# Context-Aware Focus Engine — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the Focus Engine by adding OS desktop notifications, wiring them through the existing focus policy pipeline, and enabling custom app detection rules.

**Architecture:** Layer 1 (policy wiring) and Layer 3 (process scanner) already exist. This plan adds Layer 2 (OS notifications via `tauri-plugin-notification`) and extends Layer 3 with user-customizable app rules. The notification service integrates into the existing `playNotification()` pipeline in `client/src/lib/sound/index.ts`.

**Tech Stack:** Tauri 2 (`tauri-plugin-notification`), Solid.js signals, Web Notification API (browser fallback)

**Design doc:** `docs/developer-guide/plans/2026-03-07-focus-engine-design.md`

---

## Existing Code Reference

- `client/src/lib/sound/index.ts` — Notification pipeline with 7 gates. `playNotification()` is the entry point. Focus policy is already Gate 1 (line 118).
- `client/src/stores/focus.ts` — `evaluateFocusPolicy(event)` returns `"allow"` or `"suppress"`. Already called.
- `client/src/stores/sound.ts` — Sound settings, DND check, channel notification levels.
- `client/src/stores/preferences.ts` — Cross-device preferences sync. `DEFAULT_PREFERENCES` at line 81.
- `client/src/lib/types.ts` — `UserPreferences` interface at line 732, `FocusPreferences` at line 719.
- `client/src/stores/websocket.ts` — `handleMessageNotification()` at line 122 creates `SoundEvent` and calls `playNotification()`.
- `client/src/components/settings/NotificationSettings.tsx` — Sound settings UI (270 lines).
- `client/src/components/settings/FocusSettings.tsx` — Focus mode management UI (654 lines).
- `client/src-tauri/src/lib.rs` — Tauri app setup, plugin registration at line 54.
- `client/src-tauri/Cargo.toml` — Rust dependencies.
- `client/src-tauri/capabilities/default.json` — Tauri capability permissions.
- `client/src-tauri/src/presence/` — Process scanner (scanner.rs, service.rs, games.rs) with 15s polling.
- `client/src-tauri/resources/games.json` — 15-entry game/app database.

---

## Task 1: Add Notification Preferences to Types and Defaults

**Files:**
- Modify: `client/src/lib/types.ts:719-722` (FocusPreferences), `client/src/lib/types.ts:732-775` (UserPreferences)
- Modify: `client/src/stores/preferences.ts:72-108` (defaults)

**Step 1: Add notification preferences to UserPreferences interface**

In `client/src/lib/types.ts`, add `NotificationPreferences` interface before `UserPreferences` and add `custom_app_rules` to `FocusPreferences`:

```typescript
// Add before UserPreferences (around line 731):
export interface NotificationPreferences {
  os_enabled: boolean;
  show_content: boolean;
  flash_taskbar: boolean;
}

// Add to FocusPreferences (line 719):
export interface FocusPreferences {
  modes: FocusMode[];
  auto_activate_global: boolean;
  custom_app_rules: Record<string, FocusTriggerCategory>; // process name -> category
}

// Add to UserPreferences (before onboarding_completed):
  // Desktop notification preferences
  notifications: NotificationPreferences;
```

**Step 2: Add defaults in preferences store**

In `client/src/stores/preferences.ts`, update `DEFAULT_FOCUS_PREFERENCES` and `DEFAULT_PREFERENCES`:

```typescript
// Update DEFAULT_FOCUS_PREFERENCES (line 72):
export const DEFAULT_FOCUS_PREFERENCES: FocusPreferences = {
  modes: DEFAULT_FOCUS_MODES,
  auto_activate_global: false,
  custom_app_rules: {},
};

// Add to DEFAULT_PREFERENCES (after focus, before onboarding_completed):
  notifications: {
    os_enabled: true,
    show_content: true,
    flash_taskbar: true,
  },
```

**Step 3: Commit**

```
feat(client): add notification and custom app rule preference types
```

---

## Task 2: Add `tauri-plugin-notification` Dependency

**Files:**
- Modify: `client/src-tauri/Cargo.toml`
- Modify: `client/src-tauri/src/lib.rs:54`
- Modify: `client/src-tauri/capabilities/default.json`

**Step 1: Add Rust dependency**

In `client/src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
tauri-plugin-notification = "2"
```

**Step 2: Register plugin in Tauri setup**

In `client/src-tauri/src/lib.rs`, add the plugin after `tauri_plugin_shell` (line 54):

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
```

**Step 3: Add notification capability**

In `client/src-tauri/capabilities/default.json`, add to the `permissions` array:

```json
"notification:default"
```

**Step 4: Build to verify compilation**

Run: `cd client && SQLX_OFFLINE=true cargo check -p vc-client`
Expected: Compiles without errors.

**Step 5: Commit**

```
feat(client): add tauri-plugin-notification dependency
```

---

## Task 3: Create OS Notification Service

**Files:**
- Create: `client/src/lib/notifications.ts`
- Create: `client/src/__tests__/notifications.test.ts`

**Step 1: Write the test**

Create `client/src/__tests__/notifications.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock tauri plugin
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

// Mock preferences
vi.mock("@/stores/preferences", () => ({
  getPreference: vi.fn(),
}));

describe("notifications", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  describe("formatNotificationContent", () => {
    it("returns generic body when show_content is false", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "message_dm", channelId: "ch1", isDm: true },
        { username: "Alice", content: "Hey there!", guildName: null, channelName: null },
        false,
      );
      expect(result.title).toBe("Alice");
      expect(result.body).toBe("New message");
    });

    it("returns message preview for DM when show_content is true", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "message_dm", channelId: "ch1", isDm: true },
        { username: "Alice", content: "Hey there!", guildName: null, channelName: null },
        true,
      );
      expect(result.title).toBe("Alice");
      expect(result.body).toBe("Hey there!");
    });

    it("returns mention format for channel mentions", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "message_mention", channelId: "ch1", isDm: false, mentionType: "direct" },
        { username: "Bob", content: "Check this out", guildName: "Dev Team", channelName: "general" },
        true,
      );
      expect(result.title).toBe("#general in Dev Team");
      expect(result.body).toBe("@Bob: Check this out");
    });

    it("truncates long message content", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const longContent = "a".repeat(200);
      const result = formatNotificationContent(
        { type: "message_dm", channelId: "ch1", isDm: true },
        { username: "Alice", content: longContent, guildName: null, channelName: null },
        true,
      );
      expect(result.body.length).toBeLessThanOrEqual(103); // 100 + "..."
    });

    it("returns generic body for encrypted messages", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "message_dm", channelId: "ch1", isDm: true },
        { username: "Alice", content: null, guildName: null, channelName: null },
        true,
      );
      expect(result.body).toBe("New message");
    });

    it("formats thread reply notifications", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "message_thread", channelId: "ch1", isDm: false },
        { username: "Carol", content: "I agree", guildName: "Dev Team", channelName: "general" },
        true,
      );
      expect(result.title).toBe("Thread reply in #general");
      expect(result.body).toBe("Carol: I agree");
    });

    it("formats incoming call notifications", async () => {
      const { formatNotificationContent } = await import("@/lib/notifications");
      const result = formatNotificationContent(
        { type: "call_incoming", channelId: "ch1", isDm: true },
        { username: "Dave", content: null, guildName: null, channelName: null },
        true,
      );
      expect(result.title).toBe("Incoming call");
      expect(result.body).toBe("Dave is calling you");
    });
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd client && bun run test:run -- --reporter=verbose src/__tests__/notifications.test.ts`
Expected: FAIL — `@/lib/notifications` module not found.

**Step 3: Write the notification service**

Create `client/src/lib/notifications.ts`:

```typescript
/**
 * OS Notification Service
 *
 * Sends native desktop notifications via tauri-plugin-notification (Tauri)
 * or the Web Notification API (browser). Respects user preferences for
 * content visibility and integrates with the focus policy pipeline.
 */

import type { SoundEvent, SoundEventType } from "./sound/types";

// ============================================================================
// Types
// ============================================================================

export interface NotificationContext {
  username: string;
  content: string | null;
  guildName: string | null;
  channelName: string | null;
}

interface FormattedNotification {
  title: string;
  body: string;
}

// ============================================================================
// Platform Detection
// ============================================================================

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

// ============================================================================
// State
// ============================================================================

let permissionGranted = false;
let permissionChecked = false;

// ============================================================================
// Permission Management
// ============================================================================

/**
 * Check and request notification permission.
 * Call once after user login.
 */
export async function initNotifications(): Promise<void> {
  if (permissionChecked) return;
  permissionChecked = true;

  if (isTauri()) {
    try {
      const { isPermissionGranted, requestPermission } = await import(
        "@tauri-apps/plugin-notification"
      );
      permissionGranted = await isPermissionGranted();
      if (!permissionGranted) {
        const permission = await requestPermission();
        permissionGranted = permission === "granted";
      }
    } catch (error) {
      console.warn("[Notifications] Failed to initialize Tauri notifications:", error);
    }
  } else if (typeof Notification !== "undefined") {
    if (Notification.permission === "granted") {
      permissionGranted = true;
    } else if (Notification.permission !== "denied") {
      const result = await Notification.requestPermission();
      permissionGranted = result === "granted";
    }
  }
}

/**
 * Reset permission state (for cleanup on logout).
 */
export function cleanupNotifications(): void {
  permissionChecked = false;
  permissionGranted = false;
}

// ============================================================================
// Content Formatting
// ============================================================================

const MAX_BODY_LENGTH = 100;

function truncate(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text;
  return text.slice(0, maxLength) + "...";
}

const GENERIC_BODIES: Partial<Record<SoundEventType, string>> = {
  message_dm: "New message",
  message_mention: "New mention",
  message_channel: "New message",
  message_thread: "New thread reply",
  call_incoming: "Incoming call",
};

/**
 * Format notification title and body based on event type and preferences.
 */
export function formatNotificationContent(
  event: SoundEvent,
  ctx: NotificationContext,
  showContent: boolean,
): FormattedNotification {
  const genericBody = GENERIC_BODIES[event.type] ?? "New notification";

  // No content available (encrypted or missing) — always generic
  if (!ctx.content && event.type !== "call_incoming") {
    return { title: ctx.username, body: genericBody };
  }

  // User disabled content preview — show generic
  if (!showContent && event.type !== "call_incoming") {
    const title =
      event.type === "message_mention" && ctx.channelName && ctx.guildName
        ? `#${ctx.channelName} in ${ctx.guildName}`
        : ctx.username;
    return { title, body: genericBody };
  }

  switch (event.type) {
    case "message_dm":
      return {
        title: ctx.username,
        body: truncate(ctx.content!, MAX_BODY_LENGTH),
      };

    case "message_mention":
      return {
        title:
          ctx.channelName && ctx.guildName
            ? `#${ctx.channelName} in ${ctx.guildName}`
            : ctx.username,
        body: truncate(`@${ctx.username}: ${ctx.content!}`, MAX_BODY_LENGTH),
      };

    case "message_channel":
      return {
        title:
          ctx.channelName && ctx.guildName
            ? `#${ctx.channelName} in ${ctx.guildName}`
            : ctx.username,
        body: truncate(`${ctx.username}: ${ctx.content!}`, MAX_BODY_LENGTH),
      };

    case "message_thread":
      return {
        title: ctx.channelName ? `Thread reply in #${ctx.channelName}` : "Thread reply",
        body: truncate(`${ctx.username}: ${ctx.content!}`, MAX_BODY_LENGTH),
      };

    case "call_incoming":
      return {
        title: "Incoming call",
        body: `${ctx.username} is calling you`,
      };

    default:
      return { title: ctx.username, body: genericBody };
  }
}

// ============================================================================
// Send Notification
// ============================================================================

/**
 * Send an OS notification. Only sends when the window is not focused.
 */
export async function sendOsNotification(
  event: SoundEvent,
  ctx: NotificationContext,
  showContent: boolean,
): Promise<void> {
  // Only notify when window is not focused
  if (!document.hidden) return;

  // Permission not granted
  if (!permissionGranted) return;

  const { title, body } = formatNotificationContent(event, ctx, showContent);

  if (isTauri()) {
    try {
      const { sendNotification } = await import("@tauri-apps/plugin-notification");
      sendNotification({ title, body });
    } catch (error) {
      console.warn("[Notifications] Failed to send Tauri notification:", error);
    }
  } else if (typeof Notification !== "undefined") {
    try {
      new Notification(title, { body, tag: `kaiku-${event.channelId}` });
    } catch (error) {
      console.warn("[Notifications] Failed to send web notification:", error);
    }
  }
}
```

**Step 4: Run test to verify it passes**

Run: `cd client && bun run test:run -- --reporter=verbose src/__tests__/notifications.test.ts`
Expected: All 7 tests PASS.

**Step 5: Commit**

```
feat(client): add OS notification service with content formatting
```

---

## Task 4: Wire OS Notifications into Notification Pipeline

**Files:**
- Modify: `client/src/lib/sound/index.ts:117-159`
- Modify: `client/src/stores/websocket.ts:122-169`
- Modify: `client/src/lib/sound/index.ts:63-94` (init)

**Step 1: Extend `playNotification()` to accept context and send OS notification**

In `client/src/lib/sound/index.ts`, add the import and modify the function:

```typescript
// Add import at top (after existing imports):
import { sendOsNotification, initNotifications, cleanupNotifications } from "@/lib/notifications";
import { getPreference } from "@/stores/preferences";

// Add NotificationContext to SoundEvent extended type:
import type { SoundEvent, SoundOption, NotificationContext } from "./sound/types";
```

Wait — `NotificationContext` is in `notifications.ts`, not `types.ts`. Import from there:

```typescript
import {
  sendOsNotification,
  initNotifications,
  cleanupNotifications,
  type NotificationContext,
} from "@/lib/notifications";
```

Add `initNotifications()` call inside `initSoundService()` (after preloading sounds, around line 76):

```typescript
  // Initialize OS notifications
  await initNotifications();
```

Add `cleanupNotifications()` call inside `cleanupSoundService()` (after stopRinging):

```typescript
  cleanupNotifications();
```

Extend `playNotification` signature to accept optional context:

```typescript
export async function playNotification(
  event: SoundEvent,
  ctx?: NotificationContext,
): Promise<void> {
```

After line 157 (`lastSoundTime = now;`), before `await playSoundInternal(event);`, add OS notification:

```typescript
  // Send OS notification if window is not focused
  if (ctx) {
    const osEnabled = getPreference("notifications")?.os_enabled ?? true;
    if (osEnabled) {
      const showContent = getPreference("notifications")?.show_content ?? true;
      sendOsNotification(event, ctx, showContent);
    }
  }
```

**Step 2: Pass notification context from `handleMessageNotification()`**

In `client/src/stores/websocket.ts`, modify `handleMessageNotification()` to build and pass context.

Add import at top of file:

```typescript
import type { NotificationContext } from "@/lib/notifications";
```

At line 160 (before `playNotification` call), build context from the message:

```typescript
  // Build notification context for OS notifications
  const guild = isDm ? null : guildsState.guilds.find((g) =>
    g.channels?.some((c) => c.id === message.channel_id)
  );
  const notifCtx: NotificationContext = {
    username: message.author.display_name || message.author.username,
    content: message.encrypted ? null : message.content,
    guildName: guild?.name ?? null,
    channelName: channel?.name ?? null,
  };

  // Play notification
  playNotification(
    {
      type: eventType,
      channelId: message.channel_id,
      isDm,
      mentionType: message.mention_type as MentionType,
      authorId: message.author.id,
      content: message.content,
    },
    notifCtx,
  );
```

Note: Also update the thread notification handler (`handleThreadNotification`) similarly if it exists.

**Step 3: Run tests**

Run: `cd client && bun run test:run`
Expected: All existing tests pass.

**Step 4: Commit**

```
feat(client): wire OS notifications into notification pipeline
```

---

## Task 5: Update NotificationSettings UI

**Files:**
- Modify: `client/src/components/settings/NotificationSettings.tsx`

**Step 1: Add Desktop Notifications section**

After the existing Quiet Hours section (around line 258), add a new section:

```tsx
      {/* Desktop Notifications */}
      <div class="space-y-3">
        <h4 class="text-sm font-medium text-text-primary">Desktop Notifications</h4>
        <p class="text-xs text-text-secondary">
          Show system notifications when the app is in the background.
        </p>

        {/* OS notifications toggle */}
        <label class="flex items-center justify-between cursor-pointer">
          <span class="text-sm text-text-secondary">Enable desktop notifications</span>
          <input
            type="checkbox"
            checked={getPreference("notifications")?.os_enabled ?? true}
            onChange={(e) =>
              updatePreference("notifications", {
                ...(getPreference("notifications") ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                os_enabled: e.currentTarget.checked,
              })
            }
            class="accent-accent-primary"
          />
        </label>

        {/* Show content toggle */}
        <label class="flex items-center justify-between cursor-pointer">
          <span class="text-sm text-text-secondary">Show message content in notifications</span>
          <input
            type="checkbox"
            checked={getPreference("notifications")?.show_content ?? true}
            onChange={(e) =>
              updatePreference("notifications", {
                ...(getPreference("notifications") ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                show_content: e.currentTarget.checked,
              })
            }
            class="accent-accent-primary"
          />
        </label>

        {/* Flash taskbar toggle */}
        <label class="flex items-center justify-between cursor-pointer">
          <span class="text-sm text-text-secondary">Flash taskbar on notification</span>
          <input
            type="checkbox"
            checked={getPreference("notifications")?.flash_taskbar ?? true}
            onChange={(e) =>
              updatePreference("notifications", {
                ...(getPreference("notifications") ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                flash_taskbar: e.currentTarget.checked,
              })
            }
            class="accent-accent-primary"
          />
        </label>

        {/* Test notification button */}
        <button
          type="button"
          onClick={async () => {
            const { sendOsNotification } = await import("@/lib/notifications");
            await sendOsNotification(
              { type: "message_dm", channelId: "test", isDm: true },
              { username: "Kaiku", content: "This is a test notification!", guildName: null, channelName: null },
              getPreference("notifications")?.show_content ?? true,
            );
          }}
          class="px-3 py-1.5 text-xs bg-background-secondary hover:bg-background-primary rounded transition-colors"
        >
          Send test notification
        </button>
      </div>
```

**Step 2: Verify visually**

Run the client and open Settings > Notifications. Verify the new Desktop Notifications section appears with three toggles and a test button.

**Step 3: Commit**

```
feat(client): add desktop notification settings UI
```

---

## Task 6: Add Custom App Rules to FocusSettings UI

**Files:**
- Modify: `client/src/components/settings/FocusSettings.tsx`
- Modify: `client/src/stores/focus.ts` (if custom rules need to feed into scanner)

**Step 1: Add custom app rules section to FocusSettings**

After the existing auto-activate global toggle section, add a new "App Detection" subsection:

```tsx
      {/* Custom App Rules */}
      <div class="space-y-3">
        <h4 class="text-sm font-medium text-text-primary">Custom App Detection Rules</h4>
        <p class="text-xs text-text-secondary">
          Add custom process names to automatically detect activities. These supplement the built-in detection.
        </p>

        {/* Existing custom rules list */}
        <For each={Object.entries(getPreference("focus")?.custom_app_rules ?? {})}>
          {([processName, category]) => (
            <div class="flex items-center gap-2 text-sm">
              <span class="flex-1 font-mono text-text-secondary truncate">{processName}</span>
              <span class="text-xs text-text-tertiary">{category}</span>
              <button
                type="button"
                onClick={() => {
                  const rules = { ...(getPreference("focus")?.custom_app_rules ?? {}) };
                  delete rules[processName];
                  updateFocusPreference("custom_app_rules", rules);
                }}
                class="text-text-tertiary hover:text-status-error text-xs"
              >
                Remove
              </button>
            </div>
          )}
        </For>

        {/* Add new rule form */}
        <div class="flex items-center gap-2">
          <input
            type="text"
            placeholder="Process name (e.g. obs)"
            id="custom-rule-process"
            class="flex-1 px-2 py-1 text-xs bg-background-primary border border-border-primary rounded"
          />
          <select
            id="custom-rule-category"
            class="px-2 py-1 text-xs bg-background-primary border border-border-primary rounded"
          >
            <option value="game">Game</option>
            <option value="coding">Coding</option>
            <option value="listening">Listening</option>
            <option value="watching">Watching</option>
          </select>
          <button
            type="button"
            onClick={() => {
              const input = document.getElementById("custom-rule-process") as HTMLInputElement;
              const select = document.getElementById("custom-rule-category") as HTMLSelectElement;
              const processName = input.value.trim().toLowerCase();
              if (!processName) return;
              const rules = { ...(getPreference("focus")?.custom_app_rules ?? {}) };
              rules[processName] = select.value as FocusTriggerCategory;
              updateFocusPreference("custom_app_rules", rules);
              input.value = "";
            }}
            class="px-2 py-1 text-xs bg-accent-primary text-white rounded hover:bg-accent-primary/80"
          >
            Add
          </button>
        </div>
      </div>
```

Note: `updateFocusPreference` is a helper already used in FocusSettings for updating nested focus preferences. Use the same pattern as existing code.

**Step 2: Send custom rules to Tauri scanner**

The Tauri process scanner currently uses a static `games.json`. Custom rules need to be sent to the Rust side. Two approaches:

**Option A (simpler, recommended):** Keep custom rule matching in the frontend. The scanner already emits `presence:activity_changed` for games.json matches. For custom rules, add a secondary check in the frontend's `handleActivityChange()` — when the scanner doesn't match, periodically call `scan_all_processes()` and match against custom rules client-side.

**Option B:** Add a Tauri command to update the scanner's game database at runtime. More complex, requires Mutex on GamesDatabase.

Go with **Option A** for now — it's simpler and the 15s scan already provides process data.

In `client/src/stores/focus.ts`, in the activity change handler, add custom rule checking:

```typescript
// When scanner reports no activity but custom rules exist,
// check scan_all_processes() output against custom_app_rules
```

This is a lightweight addition since the scanner already refreshes processes every 15s.

**Step 3: Verify visually**

Run the client and open Settings > Focus. Verify the Custom App Detection Rules section appears with the add form and any existing rules listed.

**Step 4: Commit**

```
feat(client): add custom app detection rules to Focus settings
```

---

## Task 7: Update Design Doc Status and Roadmap

**Files:**
- Modify: `docs/developer-guide/plans/2026-03-07-focus-engine-design.md`
- Modify: `docs/developer-guide/project/roadmap.md`

**Step 1: Update design doc status**

Change `Status: Approved` to `Status: Implemented`.

**Step 2: Mark Focus Engine complete in roadmap**

In `docs/developer-guide/project/roadmap.md`, change the Focus Engine checkbox:

```markdown
- [x] **[UX] Context-Aware Focus Engine** ... ✅
```

Update the Phase 6 completion percentage accordingly.

**Step 3: Commit**

```
docs: mark Focus Engine complete in roadmap
```

---

## Summary

| Task | Description | Key Files |
|------|-------------|-----------|
| 1 | Add notification + custom rule preferences to types/defaults | `types.ts`, `preferences.ts` |
| 2 | Add `tauri-plugin-notification` dependency and setup | `Cargo.toml`, `lib.rs`, `default.json` |
| 3 | Create OS notification service with formatting | `notifications.ts` (new), tests |
| 4 | Wire OS notifications into `playNotification()` pipeline | `sound/index.ts`, `websocket.ts` |
| 5 | Update NotificationSettings UI with desktop section | `NotificationSettings.tsx` |
| 6 | Add custom app rules UI and frontend matching | `FocusSettings.tsx`, `focus.ts` |
| 7 | Update docs and roadmap | `roadmap.md`, design doc |
