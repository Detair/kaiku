/**
 * ClipboardIndicator - Small header indicator showing clipboard status
 *
 * Shows when sensitive content is on clipboard with countdown.
 * Click to clear immediately.
 */

import {
  Component,
  Show,
  createSignal,
  createEffect,
  onCleanup,
} from "solid-js";
import { Shield } from "lucide-solid";
import {
  clearClipboard,
  onClipboardStatus,
  getClipboardStatus,
  type ClipboardStatusEvent,
  type Sensitivity,
} from "@/lib/clipboard";

/**
 * Get color class for sensitivity level.
 */
function getIndicatorColor(sensitivity: Sensitivity | null): string {
  switch (sensitivity) {
    case "critical":
      return "text-danger bg-danger/20";
    case "sensitive":
      return "text-warning bg-warning/20";
    default:
      return "text-success bg-success/20";
  }
}

const ClipboardIndicator: Component = () => {
  const [status, setStatus] = createSignal<ClipboardStatusEvent | null>(null);
  const [remainingSecs, setRemainingSecs] = createSignal<number | null>(null);

  // Load initial status and listen for updates
  createEffect(() => {
    let unlisten: (() => void) | null = null;

    // Get initial status
    getClipboardStatus().then((s) => {
      setStatus(s);
      if (s.clear_in_secs !== null) {
        setRemainingSecs(s.clear_in_secs);
      }
    });

    // Listen for updates
    onClipboardStatus((event) => {
      setStatus(event);
      if (event.clear_in_secs !== null) {
        setRemainingSecs(event.clear_in_secs);
      } else if (!event.has_sensitive_content) {
        setRemainingSecs(null);
      }
    }).then((fn) => {
      unlisten = fn;
    });

    onCleanup(() => {
      unlisten?.();
    });
  });

  // Countdown timer
  createEffect(() => {
    const secs = remainingSecs();
    if (secs === null || secs <= 0) return;

    const interval = setInterval(() => {
      setRemainingSecs((prev) => {
        if (prev === null || prev <= 1) {
          clearInterval(interval);
          return null;
        }
        return prev - 1;
      });
    }, 1000);

    onCleanup(() => clearInterval(interval));
  });

  const handleClick = async () => {
    try {
      await clearClipboard();
    } catch (err) {
      console.error("Failed to clear clipboard:", err);
    }
  };

  const currentStatus = () => status();
  const isVisible = () => currentStatus()?.has_sensitive_content ?? false;

  return (
    <Show when={isVisible()}>
      <button
        onClick={handleClick}
        class={`flex items-center gap-1.5 px-2 py-1 rounded-lg text-xs font-medium transition-colors cursor-pointer ${getIndicatorColor(currentStatus()?.sensitivity ?? null)}`}
        title="Click to clear clipboard"
      >
        <Shield class="w-3.5 h-3.5" />
        <Show when={remainingSecs() !== null}>
          <span>{remainingSecs()}s</span>
        </Show>
      </button>
    </Show>
  );
};

export default ClipboardIndicator;
