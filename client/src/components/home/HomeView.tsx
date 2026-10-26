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
import { t } from "@/i18n";
import { getThemeImage } from "@/lib/themeImage";

const HomeView: Component = () => {
  return (
    <div class="flex-1 flex h-full">
      {/* Middle: Content (Friends or DM Conversation) */}
      <div class="flex-1 flex flex-col min-w-0">
        <Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
          <>
            <div class="flex flex-col items-center pt-8 pb-4">
              <img src={getThemeImage("floki_home_idle.png")} alt={t("home.main.flokiAlt")} class="w-32 h-32 object-contain opacity-80" loading="lazy" decoding="async" />
              <p class="text-sm text-text-secondary mt-2">{t("home.main.welcome")}</p>
            </div>
            <FriendsList />
          </>
        </Show>
      </div>

      {/* Right: Context Panel (hidden on smaller screens) */}
      <HomeRightPanel />
    </div>
  );
};

export default HomeView;
