/**
 * CreateGuildModal - Modal for creating new guilds
 *
 * Simple form for guild creation with name and optional description.
 */

import { Component, createSignal, Show } from "solid-js";
import { X } from "lucide-solid";
import { createGuild } from "@/stores/guilds";
import { Portal } from "solid-js/web";

interface CreateGuildModalProps {
  onClose: () => void;
}

const CreateGuildModal: Component<CreateGuildModalProps> = (props) => {
  const [name, setName] = createSignal("");
  const [description, setDescription] = createSignal("");
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();

    const guildName = name().trim();
    if (!guildName) {
      setError("Server name is required");
      return;
    }

    if (guildName.length < 2 || guildName.length > 100) {
      setError("Server name must be between 2 and 100 characters");
      return;
    }

    setIsCreating(true);
    setError(null);

    try {
      await createGuild(guildName, description().trim() || undefined);
      props.onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create server");
    } finally {
      setIsCreating(false);
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
            <h2 class="text-xl font-bold text-text-primary">Create a Server</h2>
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
              {/* Server Name */}
              <div>
                <label class="block text-sm font-semibold text-text-primary mb-2">
                  Server Name <span class="text-accent-danger">*</span>
                </label>
                <input
                  type="text"
                  value={name()}
                  onInput={(e) => setName(e.currentTarget.value)}
                  placeholder="My Awesome Server"
                  class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                  style="background-color: var(--color-surface-layer2)"
                  maxLength={100}
                  disabled={isCreating()}
                  autofocus
                />
                <p class="text-xs text-text-secondary mt-1">
                  {name().length}/100 characters
                </p>
              </div>

              {/* Description */}
              <div>
                <label class="block text-sm font-semibold text-text-primary mb-2">
                  Description{" "}
                  <span class="text-text-secondary text-xs">(optional)</span>
                </label>
                <textarea
                  value={description()}
                  onInput={(e) => setDescription(e.currentTarget.value)}
                  placeholder="Tell us about your server..."
                  class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent resize-none"
                  style="background-color: var(--color-surface-layer2)"
                  rows={3}
                  maxLength={1000}
                  disabled={isCreating()}
                />
                <p class="text-xs text-text-secondary mt-1">
                  {description().length}/1000 characters
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
              {isCreating() ? "Creating..." : "Create Server"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default CreateGuildModal;
