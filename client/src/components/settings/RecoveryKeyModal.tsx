/**
 * Recovery Key Modal
 *
 * Displays the recovery key after registration or when requested.
 * User must acknowledge saving the key before continuing.
 */

import { Component, createSignal, Show, For } from "solid-js";
import { Portal } from "solid-js/web";
import {
  Copy,
  Download,
  X,
  Shield,
  Check,
  Loader2,
  AlertTriangle,
} from "lucide-solid";
import { secureCopy } from "@/lib/clipboard";

interface RecoveryKeyModalProps {
  /** The recovery key chunks to display. */
  keyChunks: string[];
  /** Full key for copy/download. */
  fullKey: string;
  /** Whether this is the initial setup (shows skip option). */
  isInitialSetup?: boolean;
  /** Called when user confirms they saved the key. */
  onConfirm: () => void;
  /** Called when user skips (only if isInitialSetup). */
  onSkip?: () => void;
  /** Called to close the modal. */
  onClose: () => void;
  /** Error message to display. */
  error?: string | null;
  /** Whether the confirm action is in progress. */
  isLoading?: boolean;
}

const RecoveryKeyModal: Component<RecoveryKeyModalProps> = (props) => {
  const [confirmed, setConfirmed] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  const handleCopy = async () => {
    // Use secureCopy with recovery_phrase context for critical data
    // ClipboardGuard will handle auto-clear after 60s (30s in paranoid mode)
    await secureCopy(props.fullKey, "recovery_phrase");
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownload = () => {
    const blob = new Blob([props.fullKey], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "canis-recovery-key.txt";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Portal>
      <div class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50">
        <div
          class="border border-white/10 rounded-2xl w-[500px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style="background-color: var(--color-surface-layer1)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <Shield class="w-6 h-6 text-accent-primary" />
              <h2 class="text-xl font-bold text-text-primary">
                Secure Your Messages
              </h2>
            </div>
            <Show when={!props.isInitialSetup}>
              <button
                onClick={props.onClose}
                class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
              >
                <X class="w-5 h-5" />
              </button>
            </Show>
          </div>

          {/* Content */}
          <div class="p-6 space-y-6">
            <p class="text-text-secondary">
              Your messages are end-to-end encrypted. Save your recovery key to
              restore them if you lose all devices.
            </p>

            {/* Recovery Key Display */}
            <div class="bg-surface-base rounded-xl p-4 font-mono text-lg text-center">
              <div class="grid grid-cols-4 gap-2">
                <For each={props.keyChunks}>
                  {(chunk) => <span class="text-text-primary">{chunk}</span>}
                </For>
              </div>
            </div>

            {/* Action Buttons */}
            <div class="flex gap-3">
              <button
                onClick={handleCopy}
                class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
              >
                <Show when={copied()} fallback={<Copy class="w-4 h-4" />}>
                  <Check class="w-4 h-4 text-green-400" />
                </Show>
                {copied() ? "Copied!" : "Copy"}
              </button>
              <button
                onClick={handleDownload}
                class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
              >
                <Download class="w-4 h-4" />
                Download
              </button>
            </div>

            {/* Confirmation Checkbox */}
            <label class="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={confirmed()}
                onChange={(e) => setConfirmed(e.currentTarget.checked)}
                class="w-5 h-5 rounded border-white/20 bg-surface-base text-accent-primary focus:ring-accent-primary"
              />
              <span class="text-text-secondary">
                I have saved my recovery key somewhere safe
              </span>
            </label>
          </div>

          {/* Error Display */}
          <Show when={props.error}>
            <div class="mx-6 mb-0 mt-0 flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/30 rounded-lg">
              <AlertTriangle class="w-4 h-4 text-red-400 flex-shrink-0" />
              <span class="text-sm text-red-200">{props.error}</span>
            </div>
          </Show>

          {/* Footer */}
          <div class="flex gap-3 px-6 py-4 border-t border-white/10">
            <Show when={props.isInitialSetup && props.onSkip}>
              <button
                onClick={props.onSkip}
                disabled={props.isLoading}
                class="flex-1 px-4 py-2 text-text-secondary hover:text-text-primary transition-colors disabled:opacity-50"
              >
                Skip for Now
              </button>
            </Show>
            <button
              onClick={props.onConfirm}
              disabled={!confirmed() || props.isLoading}
              class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-accent-primary hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg font-medium text-white transition-colors"
            >
              <Show when={props.isLoading}>
                <Loader2 class="w-4 h-4 animate-spin" />
              </Show>
              {props.isLoading ? "Saving..." : "Continue"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default RecoveryKeyModal;
