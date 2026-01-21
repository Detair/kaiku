import { Component, For, Show, createSignal, onMount } from "solid-js";
import { Users, Info, Plus, FileText, ChevronDown } from "lucide-solid";
import { dmsState, loadDMs, selectFriendsTab } from "@/stores/dms";
import DMItem from "./DMItem";
import NewMessageModal from "./NewMessageModal";
import UserPanel from "@/components/layout/UserPanel";

const HomeSidebar: Component = () => {
  const [showNewMessage, setShowNewMessage] = createSignal(false);
  const [showDMs, setShowDMs] = createSignal(true);

  // Mock server pages for now - in a real app these might come from a store
  const serverPages = [
    { id: "rules", title: "Rules", icon: FileText },
    { id: "announcements", title: "Announcements", icon: Info },
  ];

  onMount(() => {
    loadDMs();
  });

  // Sort DMs by last_message timestamp (descending)
  const sortedDMs = () => {
    return [...dmsState.dms].sort((a, b) => {
      const timeA = a.last_message?.created_at || a.created_at;
      const timeB = b.last_message?.created_at || b.created_at;
      return new Date(timeB).getTime() - new Date(timeA).getTime();
    });
  };

  return (
    <aside class="w-[240px] flex flex-col bg-surface-layer2 border-r border-white/10 h-full z-10">
      {/* Search Bar - Consistent with Guild Sidebar */}
      <div class="px-3 py-2 mt-2">
        <input
          type="text"
          placeholder="Find conversation..."
          class="w-full px-3 py-2 rounded-xl text-sm text-text-input placeholder:text-text-secondary/50 outline-none focus:ring-2 focus:ring-accent-primary/30 border border-white/5"
          style="background-color: var(--color-surface-base)"
        />
      </div>

      {/* Separator */}
      <div class="mx-3 my-1 border-t border-white/10" />

      {/* Friends Tab */}
      <div class="relative group mx-2 mt-1">
        {/* Active Pill */}
        <div
          class="absolute -left-2 top-1/2 -translate-y-1/2 w-1 bg-white rounded-r-full transition-all duration-200"
          style={{ height: dmsState.isShowingFriends ? "20px" : "0px" }}
        />
        <button
          onClick={() => selectFriendsTab()}
          class="w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-colors"
          classList={{
            "bg-white/10 text-text-primary": dmsState.isShowingFriends,
            "hover:bg-white/5 text-text-secondary hover:text-text-primary": !dmsState.isShowingFriends,
          }}
        >
          <Users class="w-5 h-5 transition-colors" />
          <span class="font-medium">Friends</span>
        </button>
      </div>

      {/* Separator */}
      <div class="mx-3 my-2 border-t border-white/10" />

      {/* Server Information Pages */}
      <div class="mt-2 px-3 mb-2">
        <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide mb-1 px-2">
          Information
        </div>
        <div class="space-y-1">
          <For each={serverPages}>
            {(page) => (
              <div class="relative group">
                <button class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-highlight transition-colors bg-surface-base/30">
                  <page.icon class="w-4 h-4 text-text-secondary group-hover:text-text-primary transition-colors" />
                  <span class="text-sm font-medium">{page.title}</span>
                </button>
              </div>
            )}
          </For>
        </div>
      </div>

      {/* Separator */}
      <div class="mx-3 my-2 border-t border-white/10" />

      {/* Direct Messages Header */}
      <div class="flex items-center justify-between px-3 py-1 group mb-1">
        <button 
          onClick={() => setShowDMs(!showDMs())}
          class="flex items-center gap-1.5 px-1 py-1 rounded hover:bg-white/5 transition-colors flex-1 text-left"
        >
          <ChevronDown 
            class="w-3 h-3 text-text-secondary transition-transform duration-200" 
            classList={{ "-rotate-90": !showDMs() }}
          />
          <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide group-hover:text-text-primary transition-colors">
            Direct Messages
          </span>
        </button>
        
        <button
          onClick={() => setShowNewMessage(true)}
          class="p-1 rounded hover:bg-white/10 transition-colors text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100"
          title="New Message"
        >
          <Plus class="w-4 h-4" />
        </button>
      </div>

      {/* DM List */}
      <div class="flex-1 overflow-y-auto px-2 pb-2 space-y-0.5">
        <Show when={showDMs()}>
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
              <For each={sortedDMs()}>
                {(dm) => <DMItem dm={dm} />}
              </For>
            </Show>
          </Show>
        </Show>
      </div>

      {/* User Panel (Bottom) */}
      <UserPanel />

      {/* New Message Modal */}
      <Show when={showNewMessage()}>
        <NewMessageModal onClose={() => setShowNewMessage(false)} />
      </Show>
    </aside>
  );
};

export default HomeSidebar;
