/**
 * Sidebar header button that opens the guild events list.
 * Hidden while the server has scheduled events turned off.
 */

import { Component, Show } from "solid-js";
import { Calendar } from "lucide-solid";
import { t } from "@/i18n";
import { isFeatureEnabled } from "@/stores/serverFeatures";

const GuildEventsButton: Component<{ onClick: () => void }> = (props) => (
  <Show when={isFeatureEnabled("scheduled_events")}>
    <button
      onClick={() => props.onClick()}
      class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
      title={t("home.guildSidebar.events")}
    >
      <Calendar class="w-4 h-4" />
    </button>
  </Show>
);

export default GuildEventsButton;
