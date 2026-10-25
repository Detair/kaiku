/**
 * ActiveNowModule Component
 *
 * Shows friends who are currently playing games or have active status.
 * Displays activity information using ActiveActivityCard for each friend.
 */

import { Component, Show, For } from "solid-js";
import { Coffee } from "lucide-solid";
import { t } from "@/i18n";
import { getOnlineFriends } from "@/stores/friends";
import { getUserActivity } from "@/stores/presence";
import CollapsibleModule from "./CollapsibleModule";
import ActiveActivityCard from "../ActiveActivityCard";

const ActiveNowModule: Component = () => {
  // Filter online friends to only those with active game/activity
  const activeFriends = () => {
    return getOnlineFriends().filter((f) => getUserActivity(f.user_id));
  };

  return (
    <CollapsibleModule
      id="active_now"
      title={t("home.active.title")}
      badge={activeFriends().length}
    >
      <Show
        when={activeFriends().length > 0}
        fallback={
          <div class="flex flex-col items-center justify-center py-4 text-center">
            <Coffee class="w-8 h-8 text-text-secondary mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">{t("home.active.empty")}</p>
            <p class="text-xs text-text-muted mt-1">
              {t("home.active.emptyHint")}
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
    </CollapsibleModule>
  );
};

export default ActiveNowModule;
