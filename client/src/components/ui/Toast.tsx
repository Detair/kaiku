/**
 * Toast Notification System
 *
 * Provides a simple toast notification system for displaying
 * temporary messages to the user.
 *
 * ## Usage Conventions
 *
 * | Type    | Use for                                      | Duration |
 * |---------|----------------------------------------------|----------|
 * | error   | API failures, permission denials, fatal       | 8000ms   |
 * | success | User-initiated actions that complete          | 3000ms   |
 * | info    | Background events (reconnect, bot timeout)    | 5000ms   |
 * | warning | Degraded state, approaching limits            | 5000ms   |
 *
 * ## Deduplication
 *
 * Pass a stable `id` for toasts that can fire repeatedly (e.g. WebSocket
 * reconnect, rate limit warnings) to prevent toast spam. Example:
 *
 *   showToast({ type: "warning", title: "Rate limited", id: "rate-limit" })
 *
 * ## Default Durations
 *
 * If no `duration` is specified, toasts auto-dismiss after 5000ms.
 * Use `duration: 0` for persistent toasts that require manual dismissal.
 * Prefer explicit durations matching the convention table above.
 *
 * ## Reading time
 *
 * The countdown pauses while the pointer is over a toast or focus is inside
 * it (`pauseToast`/`resumeToast`), and carries on with the time that was left.
 */

import { Component, For, createSignal, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "@/i18n";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

export type ToastType = "info" | "success" | "warning" | "error";

/** Action button configuration for a toast */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastOptions {
  type: ToastType;
  title: string;
  message?: string;
  /** Duration in ms. 0 = persistent until dismissed */
  duration?: number;
  /** Unique ID for deduplication and programmatic dismissal */
  id?: string;
  /** Optional action button */
  action?: ToastAction;
}

interface ToastInstance extends ToastOptions {
  id: string;
  createdAt: number;
}

// Global toast state (toasts exported for testing)
const [toasts, setToasts] = createSignal<ToastInstance[]>([]);
export { toasts };

// Active timeouts for auto-dismiss
const dismissTimeouts = new Map<string, number>();

// Remaining time of toasts whose timer is paused (pointer over, focus inside).
const pausedRemaining = new Map<string, number>();
// When each running timer started and how long it was set for.
const timerInfo = new Map<string, { startedAt: number; remaining: number }>();

function clearTimer(id: string): void {
  const timeout = dismissTimeouts.get(id);
  if (timeout) clearTimeout(timeout);
  dismissTimeouts.delete(id);
  timerInfo.delete(id);
}

function startTimer(id: string, ms: number): void {
  clearTimer(id);
  timerInfo.set(id, { startedAt: Date.now(), remaining: ms });
  dismissTimeouts.set(
    id,
    window.setTimeout(() => dismissToast(id), ms),
  );
}

/**
 * Stop the auto-dismiss timer of a toast while someone is reading it.
 * Safe to call repeatedly; the time left is kept for `resumeToast`.
 */
export function pauseToast(id: string): void {
  const info = timerInfo.get(id);
  if (!info) return;
  const left = Math.max(info.remaining - (Date.now() - info.startedAt), 0);
  clearTimer(id);
  pausedRemaining.set(id, left);
}

/**
 * Continue the auto-dismiss timer after `pauseToast`. A toast that has no
 * paused timer (persistent, or never paused) is left alone.
 */
export function resumeToast(id: string): void {
  const left = pausedRemaining.get(id);
  if (left === undefined) return;
  pausedRemaining.delete(id);
  // Give a toast that was nearly done a moment to be read again.
  startTimer(id, Math.max(left, MIN_RESUME_MS));
}

// Maximum number of visible toasts
const MAX_TOASTS = 5;

// After the pointer or focus leaves, a toast stays at least this long.
const MIN_RESUME_MS = 1500;

/**
 * Show a toast notification.
 * If a toast with the same ID already exists, it will be replaced.
 */
export function showToast(options: ToastOptions): string {
  const id = options.id ?? crypto.randomUUID();

  // Clear existing timeout if replacing
  clearTimer(id);
  pausedRemaining.delete(id);

  const toast: ToastInstance = {
    ...options,
    id,
    createdAt: Date.now(),
  };

  // Remove existing toast with same ID and add new one
  setToasts((prev) => {
    const filtered = prev.filter((t) => t.id !== id);
    const updated = [...filtered, toast];

    // If exceeding max toasts, remove oldest ones
    if (updated.length > MAX_TOASTS) {
      const toRemove = updated.slice(0, updated.length - MAX_TOASTS);
      // Auto-dismiss oldest toasts
      toRemove.forEach((t) => {
        clearTimer(t.id);
        pausedRemaining.delete(t.id);
      });
      return updated.slice(-MAX_TOASTS);
    }

    return updated;
  });

  // Set auto-dismiss if duration > 0
  const duration = options.duration ?? 5000;
  if (duration > 0) {
    startTimer(id, duration);
  }

  return id;
}

/**
 * Dismiss a toast by ID.
 */
export function dismissToast(id: string): void {
  clearTimer(id);
  pausedRemaining.delete(id);

  setToasts((prev) => prev.filter((t) => t.id !== id));
}

/**
 * Dismiss all toasts.
 */
export function dismissAllToasts(): void {
  for (const timeout of dismissTimeouts.values()) {
    clearTimeout(timeout);
  }
  dismissTimeouts.clear();
  timerInfo.clear();
  pausedRemaining.clear();
  setToasts([]);
}

// Style mappings
// Theme tokens only: each fill has its own label colour that is tested for
// >= 4.5:1 in every theme (styles/__tests__/contrast.test.ts).
const typeStyles: Record<ToastType, string> = {
  info: "bg-accent-primary border-accent-primary text-on-accent",
  success: "bg-accent-success border-accent-success text-on-success",
  warning: "bg-accent-warning border-accent-warning text-on-warning",
  error: "bg-accent-danger-fill border-accent-danger-fill text-on-danger",
};

const typeIcons: Record<ToastType, string> = {
  info: "i-lucide-info",
  success: "i-lucide-check-circle",
  warning: "i-lucide-alert-triangle",
  error: "i-lucide-alert-circle",
};

/**
 * Individual Toast component.
 */
const ToastItem: Component<{ toast: ToastInstance }> = (props) => {
  const handleAction = () => {
    props.toast.action?.onClick();
    dismissToast(props.toast.id);
  };

  // Pause the countdown while the pointer is over the toast or focus is
  // inside it, so a message can be read and its button reached.
  let hovered = false;
  let focused = false;
  const sync = () => {
    if (hovered || focused) pauseToast(props.toast.id);
    else resumeToast(props.toast.id);
  };

  return (
    <div
      class={`
        flex items-start gap-3 px-4 py-3 rounded-lg border shadow-lg
        max-w-sm animate-slide-in
        ${typeStyles[props.toast.type]}
      `}
      role="alert"
      onMouseEnter={() => {
        hovered = true;
        sync();
      }}
      onMouseLeave={() => {
        hovered = false;
        sync();
      }}
      onFocusIn={() => {
        focused = true;
        sync();
      }}
      onFocusOut={(e) => {
        // Moving between two controls inside the toast is not leaving it.
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        focused = false;
        sync();
      }}
    >
      <span
        class={`${typeIcons[props.toast.type]} w-5 h-5 flex-shrink-0 mt-0.5`}
      />
      <div class="flex-1 min-w-0">
        <p class="font-medium text-sm">{props.toast.title}</p>
        {props.toast.message && (
          <p class="text-sm mt-0.5">{props.toast.message}</p>
        )}
        {props.toast.action && (
          <button
            type="button"
            class="mt-2 px-3 py-1 text-xs font-medium rounded border border-current hover:ring-1 hover:ring-current transition-shadow"
            onClick={handleAction}
          >
            {props.toast.action.label}
          </button>
        )}
      </div>
      <button
        type="button"
        class="flex-shrink-0 p-1 rounded hover:ring-1 hover:ring-current transition-shadow"
        onClick={() => dismissToast(props.toast.id)}
        aria-label={t("common.dismiss")}
      >
        <span class="i-lucide-x w-4 h-4" />
      </button>
    </div>
  );
};

/**
 * Toast container component.
 * Renders all active toasts in a fixed position.
 * Mount this once in your app layout.
 */
export const ToastContainer: Component = () => {
  // Cleanup timeouts on unmount
  onCleanup(() => {
    for (const timeout of dismissTimeouts.values()) {
      clearTimeout(timeout);
    }
    dismissTimeouts.clear();
    timerInfo.clear();
    pausedRemaining.clear();
  });

  return (
    <Portal>
      <div
        class="fixed bottom-4 right-4 flex flex-col gap-2"
        style={{ "z-index": MODAL_LAYER.notice }}
        data-testid="toast-container"
        aria-live="polite"
        aria-label={t("common.notifications")}
      >
        <For each={toasts().slice(-MAX_TOASTS)}>
          {(toast) => <ToastItem toast={toast} />}
        </For>
      </div>
    </Portal>
  );
};

export default ToastContainer;
