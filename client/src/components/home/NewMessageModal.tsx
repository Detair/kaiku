/**
 * NewMessageModal Component
 *
 * Modal for creating a new DM conversation.
 */

import { Component, createSignal, For, Show, createMemo } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Search, Check } from "lucide-solid";
import { friendsState, loadFriends } from "@/stores/friends";
import { loadDMs, selectDM } from "@/stores/dms";
import * as tauri from "@/lib/tauri";
import type { Friend } from "@/lib/types";

interface NewMessageModalProps {
  onClose: () => void;
}

const NewMessageModal: Component<NewMessageModalProps> = (props) => {
  const [search, setSearch] = createSignal("");
  const [selectedIds, setSelectedIds] = createSignal<string[]>([]);
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // Load friends if not already loaded
  if (friendsState.friends.length === 0) {
    loadFriends();
  }

  const filteredFriends = createMemo(() => {
    const searchLower = search().toLowerCase();
    if (!searchLower) return friendsState.friends;
    return friendsState.friends.filter(
      (f) =>
        f.username.toLowerCase().includes(searchLower) ||
        f.display_name.toLowerCase().includes(searchLower),
    );
  });

  const toggleFriend = (userId: string) => {
    setSelectedIds((prev) =>
      prev.includes(userId)
        ? prev.filter((id) => id !== userId)
        : [...prev, userId],
    );
  };

  const handleCreate = async () => {
    if (selectedIds().length === 0) return;

    setIsCreating(true);
    setError(null);

    try {
      const dm = await tauri.createDM(selectedIds());
      await loadDMs();
      selectDM(dm.channel.id);
      props.onClose();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to create conversation",
      );
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
        onClick={props.onClose}
      >
        <div
          class="border border-white/10 rounded-2xl w-full max-w-md flex flex-col max-h-[80vh]"
          style="background-color: var(--color-surface-base)"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-4 border-b border-white/10">
            <h2 class="text-lg font-bold text-text-primary">New Message</h2>
            <button
              onClick={props.onClose}
              class="p-1 rounded hover:bg-white/10 transition-colors"
            >
              <X class="w-5 h-5 text-text-secondary" />
            </button>
          </div>

          {/* Search */}
          <div class="p-4 border-b border-white/10">
            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="text"
                value={search()}
                onInput={(e) => setSearch(e.currentTarget.value)}
                placeholder="Search friends..."
                class="w-full pl-9 pr-4 py-2 border border-white/10 rounded-lg text-text-input placeholder-text-secondary focus:outline-none focus:border-accent-primary"
                style="background-color: var(--color-surface-layer1)"
              />
            </div>
            <Show when={selectedIds().length > 0}>
              <p class="mt-2 text-sm text-text-secondary">
                {selectedIds().length} friend
                {selectedIds().length > 1 ? "s" : ""} selected
                {selectedIds().length > 1 && " (Group DM)"}
              </p>
            </Show>
          </div>

          {/* Friends List */}
          <div class="flex-1 overflow-y-auto p-2">
            <Show
              when={filteredFriends().length > 0}
              fallback={
                <div class="text-center py-8 text-text-secondary">
                  {search() ? "No friends found" : "No friends to message"}
                </div>
              }
            >
              <For each={filteredFriends()}>
                {(friend) => (
                  <FriendSelectItem
                    friend={friend}
                    selected={selectedIds().includes(friend.user_id)}
                    onToggle={() => toggleFriend(friend.user_id)}
                  />
                )}
              </For>
            </Show>
          </div>

          {/* Error */}
          <Show when={error()}>
            <div
              class="mx-4 mb-2 p-3 rounded-lg text-sm"
              style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
            >
              {error()}
            </div>
          </Show>

          {/* Footer */}
          <div class="p-4 border-t border-white/10">
            <button
              onClick={handleCreate}
              disabled={selectedIds().length === 0 || isCreating()}
              class="w-full py-2 bg-accent-primary text-white rounded-lg font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isCreating()
                ? "Creating..."
                : selectedIds().length > 1
                  ? "Create Group DM"
                  : "Create DM"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

interface FriendSelectItemProps {
  friend: Friend;
  selected: boolean;
  onToggle: () => void;
}

const FriendSelectItem: Component<FriendSelectItemProps> = (props) => {
  return (
    <button
      onClick={props.onToggle}
      class="w-full flex items-center gap-3 p-2 rounded-lg hover:bg-white/5 transition-colors"
    >
      {/* Checkbox */}
      <div
        class="w-5 h-5 rounded border-2 flex items-center justify-center transition-colors"
        classList={{
          "border-accent-primary bg-accent-primary": props.selected,
          "border-white/30": !props.selected,
        }}
      >
        <Show when={props.selected}>
          <Check class="w-3 h-3 text-white" />
        </Show>
      </div>

      {/* Avatar */}
      <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
        <span class="text-xs font-semibold text-white">
          {props.friend.display_name.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Name */}
      <div class="flex-1 text-left">
        <div class="font-medium text-text-primary">
          {props.friend.display_name}
        </div>
        <div class="text-sm text-text-secondary">@{props.friend.username}</div>
      </div>
    </button>
  );
};

export default NewMessageModal;
