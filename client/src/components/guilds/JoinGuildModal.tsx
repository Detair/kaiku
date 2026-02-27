/**
 * JoinGuildModal - Modal for joining a guild via invite code or URL
 *
 * Accepts both bare invite codes (aBcD1234) and full invite URLs
 * (https://example.com/invite/aBcD1234).
 */

import { Component, createSignal, Show } from "solid-js";
import { X } from "lucide-solid";
import { joinViaInviteCode } from "@/stores/guilds";
import { Portal } from "solid-js/web";

interface JoinGuildModalProps {
  onClose: () => void;
}

/**
 * Extract invite code from either a bare code or a full URL.
 * Returns null if the input doesn't match either format.
 */
function extractInviteCode(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  // Try URL match: /invite/<code>, ignoring trailing slash and query params
  const urlMatch = trimmed.match(/\/invite\/([A-Za-z0-9]+)\/?(?:\?.*)?$/);
  if (urlMatch) return urlMatch[1];

  // Try bare invite code (8-16 char alphanumeric)
  if (/^[A-Za-z0-9]{8,16}$/.test(trimmed)) return trimmed;

  return null;
}

const JoinGuildModal: Component<JoinGuildModalProps> = (props) => {
  const [input, setInput] = createSignal("");
  const [isJoining, setIsJoining] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const inviteCode = () => extractInviteCode(input());

  const handleSubmit = async (e: Event) => {
    e.preventDefault();

    const code = inviteCode();
    if (!code) {
      setError("Please enter a valid invite code or invite link");
      return;
    }

    setIsJoining(true);
    setError(null);

    try {
      await joinViaInviteCode(code);
      props.onClose();
    } catch (err) {
      console.error("Failed to join guild via invite code:", code, err);
      setError(err instanceof Error ? err.message : "Failed to join server");
    } finally {
      setIsJoining(false);
    }
  };

  return (
    <Portal>
      <div class="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
        <div
          class="border border-white/10 rounded-2xl w-[480px] max-h-[600px] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">Join a Server</h2>
            <button
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
              aria-label="Close"
            >
              <X size={24} />
            </button>
          </div>

          {/* Content */}
          <form onSubmit={handleSubmit} class="flex-1 overflow-y-auto p-6">
            <div class="space-y-4">
              {/* Invite Input */}
              <div>
                <label class="block text-sm font-semibold text-text-primary mb-2">
                  Invite Link or Code <span class="text-accent-danger">*</span>
                </label>
                <input
                  type="text"
                  value={input()}
                  onInput={(e) => {
                    setInput(e.currentTarget.value);
                    setError(null);
                  }}
                  placeholder="https://example.com/invite/aBcD1234 or aBcD1234"
                  class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                  style="background-color: var(--color-surface-layer2)"
                  disabled={isJoining()}
                  autofocus
                />
                <p class="text-xs text-text-secondary mt-1">
                  Paste an invite link or enter an invite code
                </p>
              </div>

              {/* Error Message */}
              <Show when={error()}>
                <div
                  class="p-3 rounded-lg"
                  style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border)"
                >
                  <p class="text-sm" style="color: var(--color-error-text)">
                    {error()}
                  </p>
                </div>
              </Show>
            </div>
          </form>

          {/* Footer */}
          <div class="flex items-center justify-end gap-3 p-6 border-t border-white/10">
            <button
              type="button"
              onClick={props.onClose}
              class="px-4 py-2 text-text-primary hover:bg-surface-layer2 rounded-lg transition-colors"
              disabled={isJoining()}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              class="px-6 py-2 bg-accent-primary hover:bg-accent-primary/90 text-white font-semibold rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={isJoining() || !inviteCode()}
            >
              {isJoining() ? "Joining..." : "Join Server"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default JoinGuildModal;
