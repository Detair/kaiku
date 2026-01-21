/**
 * HomeRightPanel Component
 *
 * Context-aware right panel for Home view.
 * Shows user profile for 1:1 DM, participants for group DM.
 */

import { Component, Show, For } from "solid-js";
import { Coffee } from "lucide-solid";
import { dmsState, getSelectedDM } from "@/stores/dms";
import { getOnlineFriends } from "@/stores/friends";
import { getUserActivity } from "@/stores/presence";
import { ActivityIndicator } from "@/components/ui";
import ActiveActivityCard from "./ActiveActivityCard";

const HomeRightPanel: Component = () => {
  const dm = () => getSelectedDM();
  const isGroupDM = () => dm()?.participants && dm()!.participants.length > 1;

  // Filter friends with active status
  const activeFriends = () => {
    return getOnlineFriends().filter(f => getUserActivity(f.user_id));
  };

  // Hide on smaller screens
  return (
    <aside class="hidden xl:flex w-[360px] flex-col bg-surface-layer1 border-l border-white/10 h-full">
      <Show
        when={!dmsState.isShowingFriends && dm()}
        fallback={
          // Active Now Panel (Friends View)
          <div class="flex-1 flex flex-col p-4 overflow-y-auto">
            <h2 class="text-xl font-bold text-text-primary mb-4">Active Now</h2>
            
            <Show
              when={activeFriends().length > 0}
              fallback={
                <div class="flex flex-col items-center justify-center flex-1 text-center opacity-60">
                  <Coffee class="w-12 h-12 text-text-secondary mb-3" />
                  <h3 class="text-lg font-medium text-text-primary">It's quiet for now...</h3>
                  <p class="text-sm text-text-secondary mt-1">
                    When friends start playing games, they'll show up here!
                  </p>
                </div>
              }
            >
              <div class="space-y-3">
                <For each={activeFriends()}>
                  {(friend) => (
                    <ActiveActivityCard
                      userId={friend.user_id}
                      displayName={friend.display_name}
                      username={friend.username}
                      avatarUrl={friend.avatar_url}
                      activity={getUserActivity(friend.user_id)!}
                    />
                  )}
                </For>
              </div>
            </Show>
          </div>
        }
      >
        <Show
          when={isGroupDM()}
          fallback={
            // 1:1 DM - show user profile
            <div class="p-4">
              <div class="flex flex-col items-center">
                <div class="w-20 h-20 rounded-full bg-accent-primary flex items-center justify-center mb-3">
                  <span class="text-2xl font-bold text-surface-base">
                    {dm()?.participants[0]?.display_name?.charAt(0).toUpperCase()}
                  </span>
                </div>
                <h3 class="text-lg font-semibold text-text-primary">
                  {dm()?.participants[0]?.display_name}
                </h3>
                <p class="text-sm text-text-secondary">
                  @{dm()?.participants[0]?.username}
                </p>
                {/* Activity */}
                <Show when={dm()?.participants[0]?.user_id && getUserActivity(dm()!.participants[0].user_id)}>
                  <div class="mt-3 w-full px-3 py-2 rounded-lg bg-white/5">
                    <ActivityIndicator activity={getUserActivity(dm()!.participants[0].user_id)!} />
                  </div>
                </Show>
              </div>
            </div>
          }
        >
          {/* Group DM - show participants */}
          <div class="p-4">
            <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-3">
              Members — {dm()?.participants.length}
            </h3>
            <div class="space-y-2">
              <For each={dm()?.participants}>
                {(p) => (
                  <div class="flex items-start gap-2 py-1">
                    <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center flex-shrink-0">
                      <span class="text-xs font-semibold text-surface-base">
                        {p.display_name.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <div class="min-w-0 flex-1">
                      <span class="text-sm text-text-primary">{p.display_name}</span>
                      <Show when={p.user_id && getUserActivity(p.user_id)}>
                        <ActivityIndicator activity={getUserActivity(p.user_id)!} compact />
                      </Show>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
      </Show>
    </aside>
  );
};

export default HomeRightPanel;
