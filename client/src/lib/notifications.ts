/**
 * OS Notification Service
 *
 * Sends native desktop notifications via tauri-plugin-notification (Tauri)
 * or the Web Notification API (browser). Respects user preferences for
 * content visibility and integrates with the focus policy pipeline.
 */

import type { SoundEvent, SoundEventType } from "./sound/types";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@/lib/tauri/common";

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
// State
// ============================================================================

let permissionGranted = false;
let permissionChecked = false;

// ============================================================================
// Focus
// ============================================================================

/**
 * Whether Kaiku has keyboard focus. A window that is visible but not focused
 * (e.g. open on a second screen next to a game) counts as unfocused, which
 * `document.hidden` cannot tell apart.
 */
export async function isAppFocused(): Promise<boolean> {
  if (isTauri()) {
    try {
      return await getCurrentWindow().isFocused();
    } catch (error) {
      console.warn("[Notifications] Failed to read window focus:", error);
    }
  }
  return typeof document !== "undefined" && document.hasFocus();
}

// ============================================================================
// Permission Management
// ============================================================================

/**
 * Check notification permission. Tauri asks the OS; browsers are never asked
 * here (see below). Call once after user login.
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
  } else if (getBrowserNotificationSupport() === "supported") {
    // Browsers only honour a permission request made from a user gesture
    // (Chrome shows an unsolicited prompt, Safari/Firefox ignore it), so never
    // ask here. Settings offers an "Allow notifications" button instead.
    permissionGranted = Notification.permission === "granted";
  }
}

export type BrowserNotificationPermission =
  | "granted"
  | "default"
  | "denied"
  | "unsupported";

/**
 * Whether this browser can show notifications via `new Notification()`.
 * iOS Safari outside a PWA has no `Notification` object, and Android Chrome
 * only allows notifications through a service worker (the constructor throws).
 */
export function getBrowserNotificationSupport(): "supported" | "unsupported" {
  if (typeof Notification === "undefined") return "unsupported";
  if (typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent)) {
    return "unsupported";
  }
  return "supported";
}

/** Current browser notification permission (browser mode only). */
export function getBrowserNotificationPermission(): BrowserNotificationPermission {
  if (getBrowserNotificationSupport() === "unsupported") return "unsupported";
  return Notification.permission;
}

/**
 * Ask the browser for notification permission. Must be called from a user
 * gesture (e.g. a click handler).
 */
export async function requestBrowserNotificationPermission(): Promise<BrowserNotificationPermission> {
  if (getBrowserNotificationSupport() === "unsupported") return "unsupported";
  try {
    const result = await Notification.requestPermission();
    permissionGranted = result === "granted";
    return result;
  } catch (error) {
    console.warn("[Notifications] Permission request failed:", error);
    return getBrowserNotificationPermission();
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
 * Send an OS notification. By default, only sends when Kaiku is not focused
 * (see {@link isAppFocused}); while focused, the in-app sound is enough.
 * Pass `force: true` to bypass the focus check (e.g. for test notifications).
 */
export async function sendOsNotification(
  event: SoundEvent,
  ctx: NotificationContext,
  showContent: boolean,
  force = false,
): Promise<void> {
  // Only notify when Kaiku is not focused (unless forced)
  if (!force && (await isAppFocused())) return;

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
  } else if (getBrowserNotificationSupport() === "supported") {
    try {
      new Notification(title, { body, tag: `kaiku-${event.channelId}` });
    } catch (error) {
      console.warn("[Notifications] Failed to send web notification:", error);
    }
  }
}
