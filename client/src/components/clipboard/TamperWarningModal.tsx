/**
 * TamperWarningModal - Warning when clipboard content was modified
 *
 * Shown when paste is attempted but clipboard content differs from what was copied.
 * This could indicate malware modifying the clipboard.
 */

import { Component } from "solid-js";
import { Portal } from "solid-js/web";
import { AlertTriangle, X, ShieldAlert } from "lucide-solid";
import type { CopyContext } from "@/lib/clipboard";

interface TamperWarningModalProps {
  /** The context of what was originally copied */
  context: CopyContext | null;
  /** Called when user cancels (safe option) */
  onCancel: () => void;
  /** Called when user chooses to paste anyway (risky) */
  onPasteAnyway: () => void;
}

/**
 * Get display label for copy context.
 */
function getContextLabel(context: CopyContext | null): string {
  if (!context) return "Unknown content";
  if (context === "recovery_phrase") return "Recovery phrase";
  if (context === "invite_link") return "Invite link";
  if (context === "message_content") return "Message content";
  if (context === "user_id") return "User ID";
  if (typeof context === "object" && "other" in context) return context.other;
  return "Unknown content";
}

const TamperWarningModal: Component<TamperWarningModalProps> = (props) => {
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onCancel();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
      >
        <div
          class="border border-danger/30 rounded-2xl w-[450px] shadow-2xl"
          style="background-color: var(--color-surface-layer1)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="p-2 rounded-full bg-danger/20">
                <AlertTriangle class="w-6 h-6 text-danger" />
              </div>
              <h2 class="text-xl font-bold text-text-primary">
                Clipboard Modified
              </h2>
            </div>
            <button
              onClick={props.onCancel}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Content */}
          <div class="p-6 space-y-4">
            <p class="text-text-secondary">
              The clipboard content was changed after you copied it. This could
              indicate <span class="text-danger font-medium">malware</span> on
              your system attempting to hijack your clipboard.
            </p>

            <div
              class="flex gap-3 p-3 rounded-lg"
              style="background-color: var(--color-surface-base)"
            >
              <div class="text-sm">
                <p class="text-text-secondary">What you copied:</p>
                <p class="text-text-primary font-medium">
                  {getContextLabel(props.context)}
                </p>
              </div>
            </div>

            <p class="text-sm text-text-secondary">
              <strong class="text-text-primary">Recommended action:</strong>{" "}
              Cancel and investigate why your clipboard was modified.
            </p>
          </div>

          {/* Footer */}
          <div class="flex gap-3 px-6 py-4 border-t border-white/10">
            <button
              onClick={props.onCancel}
              class="flex-1 px-4 py-2.5 bg-accent-primary hover:bg-accent-primary/90 rounded-lg font-medium text-white transition-colors"
            >
              Cancel (Safe)
            </button>
            <button
              onClick={props.onPasteAnyway}
              class="px-4 py-2.5 text-danger hover:bg-danger/10 rounded-lg font-medium transition-colors"
            >
              Paste Anyway
            </button>
          </div>

          {/* Security Info */}
          <div class="px-6 pb-4">
            <span class="flex items-center gap-1 text-xs text-text-secondary">
              <ShieldAlert class="w-3 h-3" />
              Clipboard content was modified after copying
            </span>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default TamperWarningModal;
