/**
 * HomeView Component
 *
 * Three-column layout for Home view (when no guild selected).
 */

import { Component, Show } from "solid-js";
import { dmsState } from "@/stores/dms";
import { FriendsList } from "@/components/social";
import DMConversation from "./DMConversation";
import HomeRightPanel from "./HomeRightPanel";

const HomeView: Component = () => {
  return (
    <div class="flex-1 flex h-full">
      {/* Middle: Content (Friends or DM Conversation) */}
      <div class="flex-1 flex flex-col min-w-0">
        <Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
          <FriendsList />
        </Show>
      </div>

      {/* Right: Context Panel (hidden on smaller screens) */}
      <HomeRightPanel />
    </div>
  );
};

export default HomeView;
