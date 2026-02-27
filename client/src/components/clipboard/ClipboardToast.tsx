/**
 * ClipboardToast - Shows copy confirmation with auto-clear countdown
 */

import {
  Component,
  Show,
  createSignal,
  createEffect,
  onCleanup,
} from "solid-js";
import { Shield, Clock, X, Plus } from "lucide-solid";
import {
  clearClipboard,
  extendClipboardTimeout,
  onClipboardStatus,
  type ClipboardStatusEvent,
  type Sensitivity,
} from "@/lib/clipboard";

interface ClipboardToastProps {
  /** Initial event that triggered the toast */
  initialEvent?: ClipboardStatusEvent;
  /** Called when toast should close */
  onClose: () => void;
}

/**
 * Get display label for sensitivity level.
 */
function getSensitivityLabel(sensitivity: Sensitivity): string {
  switch (sensitivity) {
    case "critical":
      return "Recovery phrase";
    case "sensitive":
      return "Sensitive data";
    default:
      return "Copied";
  }
}

/**
 * Get color class for sensitivity level.
 */
function getSensitivityColor(sensitivity: Sensitivity): string {
  switch (sensitivity) {
    case "critical":
      return "text-danger";
    case "sensitive":
      return "text-warning";
    default:
      return "text-success";
  }
}

const ClipboardToast: Component<ClipboardToastProps> = (props) => {
  const [status, setStatus] = createSignal<ClipboardStatusEvent | null>(
    props.initialEvent ?? null,
  );
  const [remainingSecs, setRemainingSecs] = createSignal<number | null>(
    props.initialEvent?.clear_in_secs ?? null,
  );
  const [extending, setExtending] = createSignal(false);

  // Listen for clipboard status updates
  createEffect(() => {
    let unlisten: (() => void) | null = null;

    onClipboardStatus((event) => {
      setStatus(event);
      if (event.clear_in_secs !== null) {
        setRemainingSecs(event.clear_in_secs);
      }

      // Close toast when clipboard is cleared
      if (!event.has_sensitive_content) {
        props.onClose();
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
          return 0;
        }
        return prev - 1;
      });
    }, 1000);

    onCleanup(() => clearInterval(interval));
  });

  const handleExtend = async () => {
    setExtending(true);
    try {
      const newSecs = await extendClipboardTimeout(30);
      setRemainingSecs(newSecs);
    } catch (err) {
      console.error("Failed to extend timeout:", err);
    } finally {
      setExtending(false);
    }
  };

  const handleClear = async () => {
    try {
      await clearClipboard();
      props.onClose();
    } catch (err) {
      console.error("Failed to clear clipboard:", err);
    }
  };

  const currentStatus = () => status();
  const sensitivity = () => currentStatus()?.sensitivity ?? "normal";
  const progressPercent = () => {
    const initial = props.initialEvent?.clear_in_secs ?? 60;
    const remaining = remainingSecs() ?? 0;
    return (remaining / initial) * 100;
  };

  return (
    <Show when={currentStatus()?.has_sensitive_content}>
      <div
        class="fixed bottom-4 right-4 z-50 w-80 rounded-xl shadow-lg border border-white/10 overflow-hidden"
        style="background-color: var(--color-surface-layer1)"
      >
        {/* Header */}
        <div class="flex items-center justify-between px-4 py-3">
          <div class="flex items-center gap-2">
            <Shield class={`w-5 h-5 ${getSensitivityColor(sensitivity())}`} />
            <div>
              <p class="text-sm font-medium text-text-primary">
                Copied securely
              </p>
              <p class={`text-xs ${getSensitivityColor(sensitivity())}`}>
                {getSensitivityLabel(sensitivity())}
              </p>
            </div>
          </div>
          <button
            onClick={props.onClose}
            class="p-1 text-text-secondary hover:text-text-primary rounded transition-colors"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        {/* Timer section */}
        <Show when={remainingSecs() !== null && remainingSecs()! > 0}>
          <div class="px-4 pb-3">
            <div class="flex items-center gap-2 text-xs text-text-secondary mb-2">
              <Clock class="w-3 h-3" />
              <span>Auto-clears in {remainingSecs()}s</span>
            </div>

            {/* Progress bar */}
            <div
              class="h-1.5 rounded-full overflow-hidden"
              style="background-color: var(--color-surface-base)"
            >
              <div
                class="h-full rounded-full transition-all duration-1000"
                classList={{
                  "bg-danger": sensitivity() === "critical",
                  "bg-warning": sensitivity() === "sensitive",
                  "bg-success": sensitivity() === "normal",
                }}
                style={{ width: `${progressPercent()}%` }}
              />
            </div>

            {/* Action buttons */}
            <div class="flex gap-2 mt-3">
              <button
                onClick={handleExtend}
                disabled={extending()}
                class="flex-1 flex items-center justify-center gap-1 px-3 py-1.5 text-xs font-medium rounded-lg transition-colors"
                classList={{
                  "opacity-50 cursor-not-allowed": extending(),
                  "bg-white/5 text-text-secondary hover:bg-white/10 hover:text-text-primary":
                    !extending(),
                }}
              >
                <Plus class="w-3 h-3" />
                Extend
              </button>
              <button
                onClick={handleClear}
                class="flex-1 px-3 py-1.5 text-xs font-medium text-danger bg-danger/10 hover:bg-danger/20 rounded-lg transition-colors"
              >
                Clear Now
              </button>
            </div>
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default ClipboardToast;
