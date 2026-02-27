/**
 * HomeRightPanel Component
 *
 * Context-aware right panel for Home view.
 * Shows modular sidebar when in Friends view, user profile for DMs.
 */

import { Component, Show, For } from "solid-js";
import { dmsState, getSelectedDM } from "@/stores/dms";
import { getUserActivity } from "@/stores/presence";
import { currentUser } from "@/stores/auth";
import { ActivityIndicator } from "@/components/ui";
import {
  ActiveNowModule,
  PendingModule,
  PinsModule,
  UnreadModule,
} from "./modules";

const HomeRightPanel: Component = () => {
  const dm = () => getSelectedDM();
  const otherParticipants = () => {
    const currentDM = dm();
    if (!currentDM) return [];
    const me = currentUser();
    return currentDM.participants.filter((p) => p.user_id !== me?.id);
  };
  const isGroupDM = () => otherParticipants().length > 1;

  return (
    <aside class="hidden xl:flex w-[360px] flex-col bg-surface-layer1 border-l border-white/10 h-full">
      <Show
        when={!dmsState.isShowingFriends && dm()}
        fallback={
          // Modular Sidebar (Friends View)
          <div class="flex-1 flex flex-col overflow-y-auto">
            <UnreadModule />
            <ActiveNowModule />
            <PendingModule />
            <PinsModule />
          </div>
        }
      >
        <Show
          when={isGroupDM()}
          fallback={
            // 1:1 DM - show other user's profile
            <div class="p-4">
              <div class="flex flex-col items-center">
                <div class="w-20 h-20 rounded-full bg-accent-primary flex items-center justify-center mb-3">
                  <span class="text-2xl font-bold text-white">
                    {otherParticipants()[0]
                      ?.display_name?.charAt(0)
                      .toUpperCase()}
                  </span>
                </div>
                <h3 class="text-lg font-semibold text-text-primary">
                  {otherParticipants()[0]?.display_name}
                </h3>
                <p class="text-sm text-text-secondary">
                  @{otherParticipants()[0]?.username}
                </p>
                {/* Activity */}
                <Show
                  when={
                    otherParticipants()[0]?.user_id &&
                    getUserActivity(otherParticipants()[0].user_id)
                  }
                >
                  <div class="mt-3 w-full px-3 py-2 rounded-lg bg-white/5">
                    <ActivityIndicator
                      activity={
                        getUserActivity(otherParticipants()[0].user_id)!
                      }
                    />
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
                      <span class="text-xs font-semibold text-white">
                        {p.display_name.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <div class="min-w-0 flex-1">
                      <span class="text-sm text-text-primary">
                        {p.display_name}
                      </span>
                      <Show when={p.user_id && getUserActivity(p.user_id)}>
                        <ActivityIndicator
                          activity={getUserActivity(p.user_id)!}
                          compact
                        />
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
