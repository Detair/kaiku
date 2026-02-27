/**
 * Block Confirm Modal
 *
 * Confirmation dialog before blocking a user.
 */

import { Component, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Ban } from "lucide-solid";
import { blockUser } from "@/stores/friends";

interface BlockConfirmModalProps {
  userId: string;
  username: string;
  displayName?: string;
  onClose: () => void;
}

const BlockConfirmModal: Component<BlockConfirmModalProps> = (props) => {
  const [isBlocking, setIsBlocking] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const name = () => props.displayName || props.username;

  const handleBlock = async () => {
    setIsBlocking(true);
    setError(null);

    try {
      await blockUser(props.userId);
      props.onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to block user");
      setIsBlocking(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) props.onClose();
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center"
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div
          class="absolute inset-0 bg-black/60 backdrop-blur-sm"
          onClick={handleBackdropClick}
        />

        <div
          class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style="background-color: var(--color-surface-layer1)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-9 h-9 rounded-lg bg-status-error/20 flex items-center justify-center">
                <Ban class="w-5 h-5 text-status-error" />
              </div>
              <h2 class="text-lg font-bold text-text-primary">Block User</h2>
            </div>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Content */}
          <div class="p-5 space-y-4">
            <p class="text-text-secondary text-sm">
              Are you sure you want to block{" "}
              <span class="text-text-primary font-medium">{name()}</span>?
            </p>
            <p class="text-text-secondary text-xs">
              They won't be able to send you messages, friend requests, or call
              you. You won't see their messages in shared channels.
            </p>

            {error() && (
              <div class="p-3 rounded-lg bg-status-error/10 border border-status-error/30 text-status-error text-sm">
                {error()}
              </div>
            )}

            <div class="flex gap-3 justify-end">
              <button
                onClick={props.onClose}
                class="px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
              >
                Cancel
              </button>
              <button
                onClick={handleBlock}
                disabled={isBlocking()}
                class="px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {isBlocking() ? "Blocking..." : "Block"}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default BlockConfirmModal;
