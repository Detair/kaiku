/**
 * HomeRightPanel Component
 *
 * Context-aware right panel for Home view.
 * Shows user profile for 1:1 DM, participants for group DM.
 */

import { Component, Show, For } from "solid-js";
import { dmsState, getSelectedDM } from "@/stores/dms";
import { getOnlineFriends } from "@/stores/friends";

const HomeRightPanel: Component = () => {
  const dm = () => getSelectedDM();
  const isGroupDM = () => dm()?.participants && dm()!.participants.length > 1;

  // Hide on smaller screens
  return (
    <aside class="hidden xl:flex w-60 flex-col bg-surface-layer1 border-l border-white/5">
      <Show
        when={!dmsState.isShowingFriends && dm()}
        fallback={
          // Friends view - show online count
          <div class="p-4">
            <div class="text-sm text-text-secondary">
              Online — {getOnlineFriends().length}
            </div>
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
                  <div class="flex items-center gap-2">
                    <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
                      <span class="text-xs font-semibold text-surface-base">
                        {p.display_name.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <span class="text-sm text-text-primary">{p.display_name}</span>
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
