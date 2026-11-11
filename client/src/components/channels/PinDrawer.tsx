/**
 * Pin Drawer
 *
 * Side panel showing pinned messages for a channel.
 * Slides in from the right alongside the message area,
 * similar to ThreadSidebar. ~320px wide.
 */

import { Component, For, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { Pin, X, ExternalLink } from "lucide-solid";
import Avatar from "@/components/ui/Avatar";
import { channelPins, isPinsLoading, unpinMessageAction } from "@/stores/channelPins";
import { formatTimestamp } from "@/lib/utils";
import { showToast } from "@/components/ui/Toast";
import type { ChannelPin } from "@/lib/types";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";

interface PinDrawerProps {
  channelId: string;
  canUnpin: boolean;
  onClose: () => void;
  onJumpToMessage: (messageId: string) => void;
}

const PinDrawer: Component<PinDrawerProps> = (props) => {
  const [, setSearchParams] = useSearchParams();

  const handleUnpin = async (pin: ChannelPin) => {
    try {
      await unpinMessageAction(props.channelId, pin.message.id);
    } catch (e) {
      showToast({
        type: "error",
        title: t("chat.pins.unpinFailed"),
        message: errorText(e, "chat.pins.unpinFailedHint"),
      });
    }
  };

  // The message list scrolls to `?highlight=` and loads older messages
  // around it when needed (same path as a search result).
  const handleJump = (messageId: string) => {
    setSearchParams({ highlight: messageId });
    props.onJumpToMessage(messageId);
  };

  return (
    <div
      data-testid="pin-drawer"
      role="complementary"
      aria-label={t("chat.pins.title")}
      class="w-80 flex-shrink-0 flex flex-col border-l border-white/5 bg-surface-layer1 h-full"
    >
      {/* Header */}
      <header class="h-12 px-4 flex items-center justify-between border-b border-white/5 shadow-sm">
        <div class="flex items-center gap-2">
          <Pin class="w-4 h-4 text-text-secondary" />
          <span class="font-semibold text-text-primary text-sm">{t("chat.pins.title")}</span>
        </div>
        <button
          onClick={() => props.onClose()}
          class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
          title={t("chat.pins.close")}
          aria-label={t("chat.pins.close")}
        >
          <X class="w-4 h-4" />
        </button>
      </header>

      {/* Content */}
      <div class="flex-1 overflow-y-auto p-3 space-y-2">
        <Show
          when={!isPinsLoading()}
          fallback={
            <div class="flex items-center justify-center py-8">
              <div class="w-5 h-5 border-2 border-accent-primary border-t-transparent rounded-full animate-spin" />
            </div>
          }
        >
          <Show
            when={channelPins().length > 0}
            fallback={
              <div class="flex flex-col items-center justify-center py-8 text-text-secondary text-sm">
                <Pin class="w-8 h-8 mb-2 opacity-50" />
                <p>{t("chat.pins.empty")}</p>
              </div>
            }
          >
            <For each={channelPins()}>
              {(pin) => (
                <div class="bg-surface-layer2 rounded-lg p-3 hover:bg-white/5 transition-colors">
                  {/* Author info */}
                  <div class="flex items-center gap-2 mb-2">
                    <Avatar
                      src={pin.message.author.avatar_url}
                      alt={pin.message.author.display_name}
                      size="xs"
                    />
                    <span class="text-sm font-medium text-text-primary truncate">
                      {pin.message.author.display_name}
                    </span>
                    <span class="text-xs text-text-secondary ml-auto flex-shrink-0">
                      {formatTimestamp(pin.pinned_at)}
                    </span>
                  </div>

                  {/* Content preview */}
                  <p class="text-sm text-text-secondary mb-2 line-clamp-3 break-words">
                    {pin.message.content}
                  </p>

                  {/* Actions */}
                  <div class="flex items-center gap-2">
                    <button
                      onClick={() => handleJump(pin.message.id)}
                      class="text-xs px-2 py-1 rounded bg-white/5 hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors flex items-center gap-1"
                    >
                      <ExternalLink class="w-3 h-3" />
                      {t("chat.pins.jump")}
                    </button>
                    <Show when={props.canUnpin}>
                      <button
                        onClick={() => handleUnpin(pin)}
                        class="text-xs px-2 py-1 rounded bg-white/5 hover:bg-red-500/20 text-text-secondary hover:text-red-400 transition-colors ml-auto"
                      >
                        {t("chat.pins.unpin")}
                      </button>
                    </Show>
                  </div>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default PinDrawer;
