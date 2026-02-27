/**
 * E2EE Setup Modal
 *
 * Modal for first-time E2EE setup when opening a DM. Shows the recovery key
 * and initializes encryption after user confirms they've saved it.
 */

import {
  Component,
  createSignal,
  createEffect,
  Show,
  For,
  onCleanup,
} from "solid-js";
import { Portal } from "solid-js/web";
import { invoke } from "@tauri-apps/api/core";
import {
  Shield,
  Copy,
  Download,
  Check,
  Loader2,
  AlertTriangle,
  X,
} from "lucide-solid";
import { secureCopy } from "@/lib/clipboard";
import { e2eeStore } from "@/stores/e2ee";
import { uploadKeys, markPrekeysPublished } from "@/lib/tauri";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

interface E2EESetupModalProps {
  /** Whether the modal is open. */
  isOpen: boolean;
  /** Called when the modal should close (success or cancel). */
  onClose: () => void;
  /** Called when E2EE setup completes successfully. */
  onSuccess?: () => void;
}

interface RecoveryKey {
  fullKey: string;
  chunks: string[];
}

/**
 * Generate a recovery key via Tauri command.
 */
async function generateRecoveryKey(): Promise<RecoveryKey> {
  if (!isTauri) {
    throw new Error("E2EE requires the native Tauri app");
  }
  const result = await invoke<{ full_key: string; chunks: string[] }>(
    "generate_recovery_key",
  );
  return {
    fullKey: result.full_key,
    chunks: result.chunks,
  };
}

const E2EESetupModal: Component<E2EESetupModalProps> = (props) => {
  const [recoveryKey, setRecoveryKey] = createSignal<RecoveryKey | null>(null);
  const [hasSaved, setHasSaved] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(false);
  const [isGenerating, setIsGenerating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [copied, setCopied] = createSignal(false);

  // Generate recovery key when modal opens
  createEffect(() => {
    if (props.isOpen && !recoveryKey() && !isGenerating()) {
      setIsGenerating(true);
      setError(null);

      generateRecoveryKey()
        .then((key) => {
          setRecoveryKey(key);
        })
        .catch((err) => {
          console.error(
            "[E2EESetupModal] Failed to generate recovery key:",
            err,
          );
          setError("Failed to generate recovery key. Please try again.");
        })
        .finally(() => {
          setIsGenerating(false);
        });
    }
  });

  // Reset state when modal closes
  createEffect(() => {
    if (!props.isOpen) {
      // Small delay to allow close animation
      const timer = setTimeout(() => {
        setRecoveryKey(null);
        setHasSaved(false);
        setError(null);
        setCopied(false);
      }, 200);
      onCleanup(() => clearTimeout(timer));
    }
  });

  const handleCopy = async () => {
    const key = recoveryKey();
    if (!key) return;

    try {
      // Use secureCopy with recovery_phrase context for critical data
      // ClipboardGuard will handle auto-clear after 60s (30s in paranoid mode)
      await secureCopy(key.fullKey, "recovery_phrase");
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("[E2EESetupModal] Failed to copy:", err);
    }
  };

  const handleDownload = () => {
    const key = recoveryKey();
    if (!key) return;

    const blob = new Blob([key.fullKey], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "canis-recovery-key.txt";
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleSetup = async () => {
    const key = recoveryKey();
    if (!key || !hasSaved()) return;

    setIsLoading(true);
    setError(null);

    try {
      // 1. Initialize E2EE with the recovery key
      const initResponse = await e2eeStore.initialize(key.fullKey);

      // 2. Upload identity keys and prekeys to the server
      await uploadKeys(
        null, // device_name (optional)
        initResponse.identity_key_ed25519,
        initResponse.identity_key_curve25519,
        initResponse.prekeys,
      );

      // 3. Mark prekeys as published
      await markPrekeysPublished();

      // 4. Refresh E2EE status
      await e2eeStore.checkStatus();

      // 5. Close modal and notify success
      props.onSuccess?.();
      props.onClose();
    } catch (err) {
      console.error("[E2EESetupModal] Failed to set up encryption:", err);
      setError(
        err instanceof Error
          ? err.message
          : "Failed to set up encryption. Please try again.",
      );
    } finally {
      setIsLoading(false);
    }
  };

  const handleBackdropClick = (e: MouseEvent) => {
    // Don't close on backdrop click if loading
    if (isLoading()) return;
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <Show when={props.isOpen}>
      <Portal>
        <div
          class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50"
          onClick={handleBackdropClick}
        >
          <div
            class="border border-white/10 rounded-2xl w-[500px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
              <div class="flex items-center gap-3">
                <Shield class="w-6 h-6 text-accent-primary" />
                <h2 class="text-xl font-bold text-text-primary">
                  Set Up End-to-End Encryption
                </h2>
              </div>
              <Show when={!isLoading()}>
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
              {/* Loading state while generating key */}
              <Show when={isGenerating()}>
                <div class="flex flex-col items-center justify-center py-8 space-y-4">
                  <Loader2 class="w-8 h-8 text-accent-primary animate-spin" />
                  <p class="text-text-secondary">Generating recovery key...</p>
                </div>
              </Show>

              {/* Main content when key is ready */}
              <Show when={recoveryKey() && !isGenerating()}>
                <p class="text-text-secondary">
                  Your messages will be end-to-end encrypted. Only you and the
                  recipients can read them. Save your recovery key to restore
                  access if you lose all your devices.
                </p>

                {/* Recovery Key Display */}
                <div class="bg-surface-base rounded-xl p-4 font-mono text-lg text-center">
                  <div class="grid grid-cols-4 gap-2">
                    <For each={recoveryKey()!.chunks}>
                      {(chunk) => (
                        <span class="text-text-primary">{chunk}</span>
                      )}
                    </For>
                  </div>
                </div>

                {/* Warning */}
                <div class="flex gap-3 p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
                  <AlertTriangle class="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
                  <div class="text-sm text-yellow-200">
                    <strong>Important:</strong> This key cannot be recovered if
                    lost. Store it securely, such as in a password manager.
                  </div>
                </div>

                {/* Action Buttons */}
                <div class="flex gap-3">
                  <button
                    onClick={handleCopy}
                    disabled={isLoading()}
                    class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 disabled:opacity-50 rounded-lg transition-colors text-text-primary"
                  >
                    <Show when={copied()} fallback={<Copy class="w-4 h-4" />}>
                      <Check class="w-4 h-4 text-green-400" />
                    </Show>
                    {copied() ? "Copied!" : "Copy"}
                  </button>
                  <button
                    onClick={handleDownload}
                    disabled={isLoading()}
                    class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 disabled:opacity-50 rounded-lg transition-colors text-text-primary"
                  >
                    <Download class="w-4 h-4" />
                    Download
                  </button>
                </div>

                {/* Confirmation Checkbox */}
                <label class="flex items-center gap-3 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={hasSaved()}
                    onChange={(e) => setHasSaved(e.currentTarget.checked)}
                    disabled={isLoading()}
                    class="w-5 h-5 rounded border-white/20 bg-surface-base text-accent-primary focus:ring-accent-primary disabled:opacity-50"
                  />
                  <span class="text-text-secondary">
                    I have saved my recovery key somewhere safe
                  </span>
                </label>
              </Show>
            </div>

            {/* Error Display */}
            <Show when={error()}>
              <div class="mx-6 mb-0 flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/30 rounded-lg">
                <AlertTriangle class="w-4 h-4 text-red-400 flex-shrink-0" />
                <span class="text-sm text-red-200">{error()}</span>
              </div>
            </Show>

            {/* Footer */}
            <div class="flex gap-3 px-6 py-4 border-t border-white/10">
              <button
                onClick={props.onClose}
                disabled={isLoading()}
                class="flex-1 px-4 py-2.5 text-text-secondary hover:text-text-primary hover:bg-white/5 rounded-lg font-medium transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={handleSetup}
                disabled={!hasSaved() || isLoading() || isGenerating()}
                class="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 bg-accent-primary hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg font-medium text-white transition-colors"
              >
                <Show when={isLoading()}>
                  <Loader2 class="w-4 h-4 animate-spin" />
                </Show>
                {isLoading() ? "Setting up..." : "Set up encryption"}
              </button>
            </div>
          </div>
        </div>
      </Portal>
    </Show>
  );
};

export default E2EESetupModal;
