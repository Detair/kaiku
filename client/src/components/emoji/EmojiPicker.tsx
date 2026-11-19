import { Component, createSignal, For, Show, onMount } from "solid-js";
import { emojiState, addEmojiStringToRecents } from "@/stores/emoji";
import { EMOJI_CATEGORIES, searchEmojis } from "@/lib/emojiData";
import { t, type MessageKey } from "@/i18n";

/** Translated heading per category id from `emojiData`; unknown ids keep their name. */
const CATEGORY_KEYS: Record<string, MessageKey> = {
  smileys: "chat.emoji.category.smileys",
  gestures: "chat.emoji.category.gestures",
  hearts: "chat.emoji.category.hearts",
  objects: "chat.emoji.category.objects",
  nature: "chat.emoji.category.nature",
  food: "chat.emoji.category.food",
};

export function categoryTitle(category: { id: string; name: string }): string {
  const key = CATEGORY_KEYS[category.id];
  return key ? t(key) : category.name;
}

interface EmojiPickerProps {
  onSelect: (emoji: string) => void;
  onClose: () => void;
  guildId?: string;
  /** Optional max height in pixels (for viewport-aware sizing) */
  maxHeight?: number;
}

const EmojiPicker: Component<EmojiPickerProps> = (props) => {
  const [search, setSearch] = createSignal("");
  let searchRef: HTMLInputElement | undefined;

  // Typing right away searches: focus the search box when the picker opens.
  onMount(() => searchRef?.focus());

  const handleSelect = (emoji: string) => {
    addEmojiStringToRecents(emoji);
    props.onSelect(emoji);
    props.onClose();
  };

  const filteredEmojis = () => {
    if (search().trim()) {
      return searchEmojis(search());
    }
    return null;
  };

  const guildEmojis = () => {
    if (!props.guildId) return [];
    return emojiState.guildEmojis[props.guildId] ?? [];
  };

  return (
    <div
      data-testid="emoji-picker"
      class="bg-surface-layer2 rounded-lg shadow-xl w-80 overflow-hidden flex flex-col border border-white/10"
      style={{
        ...(props.maxHeight
          ? { "max-height": `${props.maxHeight}px` }
          : { "max-height": "384px" }),
        "background-color": "var(--color-surface-layer2, #2A2A3C)", // Fallback to focused-hybrid color
      }}
    >
      {/* Search */}
      <div class="p-2 border-b border-white/10">
        <input
          ref={searchRef}
          data-testid="emoji-search"
          type="text"
          placeholder={t("chat.emoji.search")}
          aria-label={t("chat.emoji.search")}
          value={search()}
          onInput={(e) => setSearch(e.currentTarget.value)}
          class="w-full px-3 py-1.5 bg-surface-layer1 rounded text-sm text-text-primary placeholder:text-text-placeholder"
        />
      </div>

      {/* Emoji Grid */}
      <div class="flex-1 overflow-y-auto p-2 space-y-3">
        {/* Search Results */}
        <Show when={filteredEmojis()}>
          {(emojis) => (
            <Show
              when={emojis().length > 0}
              fallback={
                <p class="text-sm text-text-secondary text-center py-4" role="status">
                  {t("chat.emoji.noResults")}
                </p>
              }
            >
            <div class="flex flex-wrap gap-1">
              <For each={emojis()}>
                {(emoji) => (
                  <button
                    class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl transition-colors"
                    onClick={() => handleSelect(emoji)}
                  >
                    {emoji}
                  </button>
                )}
              </For>
            </div>
            </Show>
          )}
        </Show>

        {/* Normal View */}
        <Show when={!filteredEmojis()}>
          {/* Recents */}
          <Show when={emojiState.recents.length > 0}>
            <div>
              <div class="text-xs text-text-secondary uppercase mb-1 px-1">
                {t("chat.emoji.recent")}
              </div>
              <div class="flex flex-wrap gap-1">
                <For each={emojiState.recents}>
                  {(emoji) => (
                    <button
                      class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl transition-colors"
                      onClick={() => handleSelect(emoji.id)}
                    >
                      {emoji.id}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Guild Emojis */}
          <Show when={guildEmojis().length > 0}>
            <div>
              <div class="text-xs text-text-secondary uppercase mb-1 px-1">
                {t("chat.emoji.server")}
              </div>
              <div class="flex flex-wrap gap-1">
                <For each={guildEmojis()}>
                  {(emoji) => (
                    <button
                      class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded"
                      onClick={() => handleSelect(`:${emoji.name}:`)}
                      title={`:${emoji.name}:`}
                    >
                      <img
                        src={emoji.image_url}
                        alt={emoji.name}
                        class="w-6 h-6"
                      />
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Categories */}
          <For each={EMOJI_CATEGORIES}>
            {(category) => (
              <div>
                <div class="text-xs text-text-secondary uppercase mb-1 px-1">
                  {categoryTitle(category)}
                </div>
                <div class="flex flex-wrap gap-1">
                  <For each={category.emojis}>
                    {(emoji) => (
                      <button
                        class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl transition-colors"
                        onClick={() => handleSelect(emoji)}
                      >
                        {emoji}
                      </button>
                    )}
                  </For>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
};

export default EmojiPicker;
