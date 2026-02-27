/**
 * Activity Indicator Component
 *
 * Displays a user's current activity (game, music, etc.)
 * with an icon and label.
 */

import { Component, Show, Switch, Match } from "solid-js";
import { Gamepad2, Music, Monitor, Code, Sparkles } from "lucide-solid";
import type { Activity, ActivityType } from "@/lib/types";

interface ActivityIndicatorProps {
  /** The activity to display. */
  activity: Activity;
  /** Whether to show in compact mode (icon + name only). */
  compact?: boolean;
}

const activityLabels: Record<ActivityType, string> = {
  game: "Playing",
  listening: "Listening to",
  watching: "Watching",
  coding: "Coding in",
  custom: "Using",
};

const iconClass = "w-3 h-3 flex-shrink-0";

const ActivityIcon: Component<{ type: ActivityType }> = (props) => {
  return (
    <Switch fallback={<Sparkles class={iconClass} />}>
      <Match when={props.type === "game"}>
        <Gamepad2 class={iconClass} />
      </Match>
      <Match when={props.type === "listening"}>
        <Music class={iconClass} />
      </Match>
      <Match when={props.type === "watching"}>
        <Monitor class={iconClass} />
      </Match>
      <Match when={props.type === "coding"}>
        <Code class={iconClass} />
      </Match>
      <Match when={props.type === "custom"}>
        <Sparkles class={iconClass} />
      </Match>
    </Switch>
  );
};

const ActivityIndicator: Component<ActivityIndicatorProps> = (props) => {
  const label = () => activityLabels[props.activity.type] || "Using";

  return (
    <div class="flex items-center gap-1.5 text-xs text-purple-400">
      <ActivityIcon type={props.activity.type} />
      <Show when={!props.compact}>
        <span class="text-text-tertiary">{label()}</span>
      </Show>
      <span class="font-medium truncate max-w-[120px]">
        {props.activity.name}
      </span>
    </div>
  );
};

export default ActivityIndicator;
