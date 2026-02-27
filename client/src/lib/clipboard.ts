/**
 * Clipboard Protection Library
 *
 * Provides secure clipboard operations with auto-clear, tamper detection,
 * and audit logging. Uses Tauri commands when available, falls back to
 * navigator.clipboard in browser mode.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const isTauri = "__TAURI__" in window;

/** Context for what's being copied (affects sensitivity). */
export type CopyContext =
  | "recovery_phrase"
  | "invite_link"
  | "message_content"
  | "user_id"
  | { other: string };

/** Sensitivity level for copied content. */
export type Sensitivity = "critical" | "sensitive" | "normal";

/** Protection level setting. */
export type ProtectionLevel = "minimal" | "standard" | "strict";

/** Result of a copy operation. */
export interface CopyResult {
  success: boolean;
  auto_clear_in_secs: number | null;
  sensitivity: Sensitivity;
}

/** Result of a paste operation. */
export interface PasteResult {
  content: string;
  tampered: boolean;
  external: boolean;
  context: CopyContext | null;
}

/** Clipboard status event. */
export interface ClipboardStatusEvent {
  has_sensitive_content: boolean;
  clear_in_secs: number | null;
  context: CopyContext | null;
  sensitivity: Sensitivity | null;
}

/** Clipboard settings. */
export interface ClipboardSettings {
  protection_level: ProtectionLevel;
  paranoid_mode_enabled: boolean;
  show_copy_toast: boolean;
  show_status_indicator: boolean;
}

// Browser-mode state for basic protection
let browserClipboardHash: string | null = null;
let browserClipboardContext: CopyContext | null = null;
let browserClearTimeout: ReturnType<typeof setTimeout> | null = null;

/**
 * Hash content for browser mode tamper detection.
 */
async function hashContent(content: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(content);
  const hashBuffer = await crypto.subtle.digest("SHA-256", data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Get sensitivity level for a copy context.
 */
function getSensitivity(context: CopyContext): Sensitivity {
  if (context === "recovery_phrase") return "critical";
  if (context === "invite_link") return "sensitive";
  return "normal";
}

/**
 * Get auto-clear timeout for browser mode.
 */
function getBrowserTimeout(sensitivity: Sensitivity): number | null {
  switch (sensitivity) {
    case "critical":
      return 60;
    case "sensitive":
      return 120;
    default:
      return null;
  }
}

/**
 * Copy content to clipboard securely.
 */
export async function secureCopy(
  content: string,
  context: CopyContext,
): Promise<CopyResult> {
  if (isTauri) {
    return invoke<CopyResult>("secure_copy", { content, context });
  }

  // Browser fallback
  await navigator.clipboard.writeText(content);

  const sensitivity = getSensitivity(context);
  const timeoutSecs = getBrowserTimeout(sensitivity);

  // Store hash for tamper detection
  browserClipboardHash = await hashContent(content);
  browserClipboardContext = context;

  // Schedule auto-clear
  if (browserClearTimeout) {
    clearTimeout(browserClearTimeout);
  }

  if (timeoutSecs) {
    browserClearTimeout = setTimeout(async () => {
      try {
        const current = await navigator.clipboard.readText();
        const currentHash = await hashContent(current);
        if (currentHash === browserClipboardHash) {
          await navigator.clipboard.writeText("");
          browserClipboardHash = null;
          browserClipboardContext = null;
        }
      } catch {
        // Clipboard access may be denied
      }
    }, timeoutSecs * 1000);
  }

  return {
    success: true,
    auto_clear_in_secs: timeoutSecs,
    sensitivity,
  };
}

/**
 * Paste from clipboard with tamper detection.
 */
export async function securePaste(): Promise<PasteResult> {
  if (isTauri) {
    return invoke<PasteResult>("secure_paste");
  }

  // Browser fallback
  const content = await navigator.clipboard.readText();
  const currentHash = await hashContent(content);

  const tampered =
    browserClipboardHash !== null && currentHash !== browserClipboardHash;
  const external = browserClipboardHash === null;

  return {
    content,
    tampered,
    external,
    context: external ? null : browserClipboardContext,
  };
}

/**
 * Clear clipboard immediately.
 */
export async function clearClipboard(): Promise<void> {
  if (isTauri) {
    return invoke("clear_clipboard");
  }

  // Browser fallback
  await navigator.clipboard.writeText("");
  browserClipboardHash = null;
  browserClipboardContext = null;

  if (browserClearTimeout) {
    clearTimeout(browserClearTimeout);
    browserClearTimeout = null;
  }
}

/**
 * Extend the auto-clear timeout.
 */
export async function extendClipboardTimeout(
  additionalSecs: number = 30,
): Promise<number> {
  if (isTauri) {
    return invoke<number>("extend_clipboard_timeout", {
      additional_secs: additionalSecs,
    });
  }

  // Browser fallback - not fully supported
  throw new Error("Timeout extension not supported in browser mode");
}

/**
 * Get current clipboard status.
 */
export async function getClipboardStatus(): Promise<ClipboardStatusEvent> {
  if (isTauri) {
    return invoke<ClipboardStatusEvent>("get_clipboard_status");
  }

  // Browser fallback
  return {
    has_sensitive_content: browserClipboardHash !== null,
    clear_in_secs: null, // Can't track in browser mode
    context: browserClipboardContext,
    sensitivity: browserClipboardContext
      ? getSensitivity(browserClipboardContext)
      : null,
  };
}

/**
 * Update clipboard protection settings.
 */
export async function updateClipboardSettings(
  settings: ClipboardSettings,
): Promise<void> {
  if (isTauri) {
    return invoke("update_clipboard_settings", { settings });
  }
  // Browser mode doesn't persist settings
}

/**
 * Get current clipboard protection settings.
 */
export async function getClipboardSettings(): Promise<ClipboardSettings> {
  if (isTauri) {
    return invoke<ClipboardSettings>("get_clipboard_settings");
  }

  // Browser fallback - default settings
  return {
    protection_level: "standard",
    paranoid_mode_enabled: false,
    show_copy_toast: true,
    show_status_indicator: true,
  };
}

/**
 * Listen for clipboard status events.
 */
export async function onClipboardStatus(
  callback: (event: ClipboardStatusEvent) => void,
): Promise<UnlistenFn> {
  if (isTauri) {
    return listen<ClipboardStatusEvent>("clipboard-status", (event) => {
      callback(event.payload);
    });
  }

  // Browser mode - no events
  return () => {};
}

/**
 * Listen for tamper detection events.
 */
export async function onClipboardTamper(
  callback: (context: CopyContext) => void,
): Promise<UnlistenFn> {
  if (isTauri) {
    return listen<{ context: CopyContext }>(
      "clipboard-tamper-detected",
      (event) => {
        callback(event.payload.context);
      },
    );
  }

  // Browser mode - no events
  return () => {};
}
