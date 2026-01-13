/**
 * DMSidebar Component
 *
 * Left column of Home view with Friends tab and DM list.
 */

import { Component, For, Show, createSignal, onMount } from "solid-js";
import { Users, Plus } from "lucide-solid";
import { dmsState, loadDMs, selectFriendsTab } from "@/stores/dms";
import DMItem from "./DMItem";
import NewMessageModal from "./NewMessageModal";

const DMSidebar: Component = () => {
  const [showNewMessage, setShowNewMessage] = createSignal(false);

  onMount(() => {
    loadDMs();
  });

  return (
    <aside class="w-60 flex flex-col bg-surface-layer1 border-r border-white/5">
      {/* Friends Tab */}
      <button
        onClick={() => selectFriendsTab()}
        class="flex items-center gap-3 px-3 py-2 mx-2 mt-2 rounded-lg transition-colors"
        classList={{
          "bg-white/10": dmsState.isShowingFriends,
          "hover:bg-white/5": !dmsState.isShowingFriends,
        }}
      >
        <Users class="w-5 h-5 text-text-secondary" />
        <span class="font-medium text-text-primary">Friends</span>
      </button>

      {/* Separator */}
      <div class="mx-3 my-2 border-t border-white/10" />

      {/* Direct Messages Header */}
      <div class="flex items-center justify-between px-3 py-1">
        <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
          Direct Messages
        </span>
        <button
          onClick={() => setShowNewMessage(true)}
          class="p-1 rounded hover:bg-white/10 transition-colors"
          title="New Message"
        >
          <Plus class="w-4 h-4 text-text-secondary" />
        </button>
      </div>

      {/* DM List */}
      <div class="flex-1 overflow-y-auto px-2 py-1 space-y-0.5">
        <Show
          when={!dmsState.isLoading}
          fallback={
            <div class="flex items-center justify-center py-8">
              <span class="text-text-secondary text-sm">Loading...</span>
            </div>
          }
        >
          <Show
            when={dmsState.dms.length > 0}
            fallback={
              <div class="text-center py-8 px-4">
                <p class="text-text-secondary text-sm">No conversations yet</p>
                <button
                  onClick={() => setShowNewMessage(true)}
                  class="mt-2 text-accent-primary text-sm hover:underline"
                >
                  Start a conversation
                </button>
              </div>
            }
          >
            <For each={dmsState.dms}>
              {(dm) => <DMItem dm={dm} />}
            </For>
          </Show>
        </Show>
      </div>

      {/* New Message Modal */}
      <Show when={showNewMessage()}>
        <NewMessageModal onClose={() => setShowNewMessage(false)} />
      </Show>
    </aside>
  );
};

export default DMSidebar;
