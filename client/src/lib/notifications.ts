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
