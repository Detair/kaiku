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
 */

import { Component, For, createSignal, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";

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

// Maximum number of visible toasts
const MAX_TOASTS = 5;

/**
 * Show a toast notification.
 * If a toast with the same ID already exists, it will be replaced.
 */
export function showToast(options: ToastOptions): string {
  const id = options.id ?? crypto.randomUUID();

  // Clear existing timeout if replacing
  const existingTimeout = dismissTimeouts.get(id);
  if (existingTimeout) {
    clearTimeout(existingTimeout);
    dismissTimeouts.delete(id);
  }

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
        const timeout = dismissTimeouts.get(t.id);
        if (timeout) {
          clearTimeout(timeout);
          dismissTimeouts.delete(t.id);
        }
      });
      return updated.slice(-MAX_TOASTS);
    }

    return updated;
  });

  // Set auto-dismiss if duration > 0
  const duration = options.duration ?? 5000;
  if (duration > 0) {
    const timeout = window.setTimeout(() => {
      dismissToast(id);
    }, duration);
    dismissTimeouts.set(id, timeout);
  }

  return id;
}

/**
 * Dismiss a toast by ID.
 */
export function dismissToast(id: string): void {
  const timeout = dismissTimeouts.get(id);
  if (timeout) {
    clearTimeout(timeout);
    dismissTimeouts.delete(id);
  }

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
  setToasts([]);
}

// Style mappings
const typeStyles: Record<ToastType, string> = {
  info: "bg-blue-600 border-blue-500",
  success: "bg-green-600 border-green-500",
  warning: "bg-yellow-600 border-yellow-500",
  error: "bg-red-600 border-red-500",
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

  return (
    <div
      class={`
        flex items-start gap-3 px-4 py-3 rounded-lg border shadow-lg
        text-white max-w-sm animate-slide-in
        ${typeStyles[props.toast.type]}
      `}
      role="alert"
    >
      <span
        class={`${typeIcons[props.toast.type]} w-5 h-5 flex-shrink-0 mt-0.5`}
      />
      <div class="flex-1 min-w-0">
        <p class="font-medium text-sm">{props.toast.title}</p>
        {props.toast.message && (
          <p class="text-sm opacity-90 mt-0.5">{props.toast.message}</p>
        )}
        {props.toast.action && (
          <button
            type="button"
            class="mt-2 px-3 py-1 text-xs font-medium bg-white/20 rounded hover:bg-white/30 transition-colors"
            onClick={handleAction}
          >
            {props.toast.action.label}
          </button>
        )}
      </div>
      <button
        type="button"
        class="flex-shrink-0 p-1 rounded hover:bg-white/20 transition-colors"
        onClick={() => dismissToast(props.toast.id)}
        aria-label="Dismiss"
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
  });

  return (
    <Portal>
      <div
        class="fixed bottom-4 right-4 z-50 flex flex-col gap-2"
        aria-live="polite"
        aria-label="Notifications"
      >
        <For each={toasts().slice(-MAX_TOASTS)}>
          {(toast) => <ToastItem toast={toast} />}
        </For>
      </div>
    </Portal>
  );
};

export default ToastContainer;
