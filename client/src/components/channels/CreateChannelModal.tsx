/**
 * CreateChannelModal - Modal for creating new channels in a guild
 *
 * Simple form for channel creation with name and type selection.
 */

import { Component, JSX, createSignal, Show } from "solid-js";
import { X, Hash, Mic, MessagesSquare, Megaphone, Check } from "lucide-solid";
import { createChannel } from "@/stores/channels";
import { getCategory } from "@/stores/categories";
import { Portal } from "solid-js/web";
import { showToast } from "@/components/ui/Toast";
import { createDialog } from "@/lib/a11y/dialog";
import { t } from "@/i18n";
import { isFeatureEnabled } from "@/stores/serverFeatures";
import Button from "@/components/ui/Button";

/**
 * One channel-type option. Selected: accent frame, tinted fill and a check
 * badge (so the choice is not shown by a thin border alone). Locked: dashed,
 * readable "unavailable" style instead of fading out.
 */
const TypeCard: Component<{
  selected: boolean;
  locked?: boolean;
  onSelect: () => void;
  icon: JSX.Element;
  title: string;
  subtitle: string;
}> = (props) => (
  <button
    type="button"
    onClick={() => !props.locked && props.onSelect()}
    disabled={props.locked}
    aria-pressed={props.selected}
    class="relative flex-1 flex items-center gap-3 p-4 rounded-xl border-2 transition-all text-left"
    classList={{
      "border-accent-primary bg-accent-primary/20": props.selected,
      "border-border-solid hover:border-text-secondary": !props.selected && !props.locked,
      "state-unavailable": !!props.locked,
    }}
  >
    <Show when={props.selected}>
      <span
        class="absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center bg-accent-primary text-on-accent"
        aria-hidden="true"
      >
        <Check size={12} stroke-width={3.5} />
      </span>
    </Show>
    <div
      class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
      style={{ "background-color": "var(--color-surface-layer2)" }}
    >
      {props.icon}
    </div>
    <div>
      <div class="font-semibold text-text-primary">{props.title}</div>
      <div class="text-xs text-text-secondary">{props.subtitle}</div>
    </div>
  </button>
);

interface CreateChannelModalProps {
  guildId: string;
  initialType?: "text" | "voice";
  /** Optional category ID to create the channel in */
  categoryId?: string | null;
  onClose: () => void;
  onCreated?: (channelId: string) => void;
}

const CreateChannelModal: Component<CreateChannelModalProps> = (props) => {
  const dialog = createDialog({ onClose: () => props.onClose() });

  // If the category restricts channel type, lock the selector
  const categoryTypeRestriction = () => {
    if (!props.categoryId) return null;
    const cat = getCategory(props.categoryId);
    if (!cat || cat.category_type === "mixed") return null;
    return cat.category_type as "text" | "voice";
  };

  const [name, setName] = createSignal("");
  const [channelType, setChannelType] = createSignal<
    "text" | "voice" | "forum" | "announcement"
  >(categoryTypeRestriction() ?? props.initialType ?? "text");
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const isTypeLocked = () => categoryTypeRestriction() !== null;

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
      <div
        class="fixed inset-0 bg-black/60 flex items-center justify-center z-50"
        onClick={(e) => {
          if (e.target === e.currentTarget) props.onClose();
        }}
      >
        <div
          {...dialog.dialogProps}
          ref={dialog.ref}
          class="border border-white/10 rounded-2xl w-[440px] flex flex-col shadow-2xl"
          style={{"background-color":"var(--color-surface-base)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 id={dialog.titleId} class="text-xl font-bold text-text-primary">Create Channel</h2>
            <button
              type="button"
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
              aria-label={t("common.close")}
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
                  <TypeCard
                    selected={channelType() === "text"}
                    locked={isTypeLocked() && channelType() !== "text"}
                    onSelect={() => setChannelType("text")}
                    icon={<Hash size={20} class="text-text-primary" />}
                    title="Text"
                    subtitle="Send messages"
                  />
                  <TypeCard
                    selected={channelType() === "voice"}
                    locked={isTypeLocked() && channelType() !== "voice"}
                    onSelect={() => setChannelType("voice")}
                    icon={<Mic size={20} class="text-text-primary" />}
                    title="Voice"
                    subtitle="Talk with others"
                  />
                  <Show when={!isTypeLocked() && isFeatureEnabled("forum_channels")}>
                    <TypeCard
                      selected={channelType() === "forum"}
                      onSelect={() => setChannelType("forum")}
                      icon={<MessagesSquare size={20} class="text-text-primary" />}
                      title="Forum"
                      subtitle="Organized posts"
                    />
                  </Show>
                  <Show when={!isTypeLocked() && isFeatureEnabled("announcement_channels")}>
                    <TypeCard
                      selected={channelType() === "announcement"}
                      onSelect={() => setChannelType("announcement")}
                      icon={<Megaphone size={20} class="text-text-primary" />}
                      title="Announcement"
                      subtitle="Broadcast + follow"
                    />
                  </Show>
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
                    data-testid="create-channel-name"
                    value={name()}
                    onInput={(e) =>
                      setName(
                        e.currentTarget.value
                          .toLowerCase()
                          .replace(/\s+/g, "-"),
                      )
                    }
                    placeholder={
                      channelType() === "voice"
                        ? t("channels.create.namePlaceholderVoice")
                        : t("channels.create.namePlaceholderText")
                    }
                    class="w-full pl-11 pr-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                    style={{"background-color":"var(--color-surface-layer2)"}}
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
                  style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)"}}
                >
                  <p class="text-sm" style={{"color":"var(--color-error-text)"}}>
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
              class="px-4 py-2 text-text-primary hover:bg-surface-layer2 rounded-lg transition-colors state-disabled-quiet"
              disabled={isCreating()}
            >
              Cancel
            </button>
            <Button
              type="submit"
              onClick={handleSubmit}
              data-testid="create-channel-submit"
              disabled={isCreating() || !name().trim()}
              disabledReason={!name().trim() && t("channels.create.nameRequired")}
            >
              {isCreating() ? "Creating..." : "Create Channel"}
            </Button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default CreateChannelModal;
