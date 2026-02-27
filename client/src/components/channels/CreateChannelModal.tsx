/**
 * CreateChannelModal - Modal for creating new channels in a guild
 *
 * Simple form for channel creation with name and type selection.
 */

import { Component, createSignal, Show } from "solid-js";
import { X, Hash, Mic } from "lucide-solid";
import { createChannel } from "@/stores/channels";
import { Portal } from "solid-js/web";
import { showToast } from "@/components/ui/Toast";

interface CreateChannelModalProps {
  guildId: string;
  initialType?: "text" | "voice";
  /** Optional category ID to create the channel in */
  categoryId?: string | null;
  onClose: () => void;
  onCreated?: (channelId: string) => void;
}

const CreateChannelModal: Component<CreateChannelModalProps> = (props) => {
  const [name, setName] = createSignal("");
  const [channelType, setChannelType] = createSignal<"text" | "voice">(
    props.initialType || "text",
  );
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();

    const channelName = name().trim();
    if (!channelName) {
      setError("Channel name is required");
      return;
    }

    if (channelName.length < 1 || channelName.length > 64) {
      setError("Channel name must be between 1 and 64 characters");
      return;
    }

    setIsCreating(true);
    setError(null);

    try {
      const channel = await createChannel(
        channelName,
        channelType(),
        props.guildId,
        undefined, // topic
        props.categoryId ?? undefined,
      );
      showToast({
        type: "success",
        title: "Channel Created",
        message: `#${channelName} has been created successfully.`,
        duration: 3000,
      });
      props.onCreated?.(channel.id);
      props.onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create channel");
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Portal>
      <div class="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
        <div
          class="border border-white/10 rounded-2xl w-[440px] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">Create Channel</h2>
            <button
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
              aria-label="Close"
            >
              <X size={24} />
            </button>
          </div>

          {/* Content */}
          <form onSubmit={handleSubmit} class="flex-1 p-6">
            <div class="space-y-5">
              {/* Channel Type */}
              <div>
                <label class="block text-sm font-semibold text-text-primary mb-3">
                  Channel Type
                </label>
                <div class="flex gap-3">
                  <button
                    type="button"
                    onClick={() => setChannelType("text")}
                    class="flex-1 flex items-center gap-3 p-4 rounded-xl border-2 transition-all"
                    classList={{
                      "border-accent-primary bg-accent-primary/10":
                        channelType() === "text",
                      "border-white/10 hover:border-white/20":
                        channelType() !== "text",
                    }}
                  >
                    <div
                      class="w-10 h-10 rounded-lg flex items-center justify-center"
                      style="background-color: var(--color-surface-layer2)"
                    >
                      <Hash size={20} class="text-text-primary" />
                    </div>
                    <div class="text-left">
                      <div class="font-semibold text-text-primary">Text</div>
                      <div class="text-xs text-text-secondary">
                        Send messages
                      </div>
                    </div>
                  </button>
                  <button
                    type="button"
                    onClick={() => setChannelType("voice")}
                    class="flex-1 flex items-center gap-3 p-4 rounded-xl border-2 transition-all"
                    classList={{
                      "border-accent-primary bg-accent-primary/10":
                        channelType() === "voice",
                      "border-white/10 hover:border-white/20":
                        channelType() !== "voice",
                    }}
                  >
                    <div
                      class="w-10 h-10 rounded-lg flex items-center justify-center"
                      style="background-color: var(--color-surface-layer2)"
                    >
                      <Mic size={20} class="text-text-primary" />
                    </div>
                    <div class="text-left">
                      <div class="font-semibold text-text-primary">Voice</div>
                      <div class="text-xs text-text-secondary">
                        Talk with others
                      </div>
                    </div>
                  </button>
                </div>
              </div>

              {/* Channel Name */}
              <div>
                <label class="block text-sm font-semibold text-text-primary mb-2">
                  Channel Name <span class="text-accent-danger">*</span>
                </label>
                <div class="relative">
                  <div class="absolute left-4 top-1/2 -translate-y-1/2 text-text-secondary">
                    {channelType() === "text" ? (
                      <Hash size={18} />
                    ) : (
                      <Mic size={18} />
                    )}
                  </div>
                  <input
                    type="text"
                    value={name()}
                    onInput={(e) =>
                      setName(
                        e.currentTarget.value
                          .toLowerCase()
                          .replace(/\s+/g, "-"),
                      )
                    }
                    placeholder={
                      channelType() === "text" ? "general-chat" : "voice-lounge"
                    }
                    class="w-full pl-11 pr-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                    style="background-color: var(--color-surface-layer2)"
                    maxLength={64}
                    disabled={isCreating()}
                    autofocus
                  />
                </div>
                <p class="text-xs text-text-secondary mt-1">
                  {name().length}/64 characters
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
              disabled={isCreating()}
            >
              Cancel
            </button>
            <button
              type="submit"
              onClick={handleSubmit}
              class="px-6 py-2 bg-accent-primary hover:bg-accent-primary/90 text-white font-semibold rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={isCreating() || !name().trim()}
            >
              {isCreating() ? "Creating..." : "Create Channel"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default CreateChannelModal;
