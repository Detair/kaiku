import { Component, Show } from "solid-js";
import { Activity } from "@/lib/types";
import { ActivityIndicator } from "@/components/ui";

interface ActiveActivityCardProps {
  displayName: string;
  username: string;
  avatarUrl?: string | null;
  activity: Activity;
  userId: string;
}

const ActiveActivityCard: Component<ActiveActivityCardProps> = (props) => {
  return (
    <div class="p-3 bg-surface-layer2 rounded-xl border border-white/5 hover:border-white/10 transition-colors">
      <div class="flex items-center gap-3 mb-2">
        {/* Avatar */}
        <div class="relative w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center text-xs font-bold text-white">
          <Show
            when={props.avatarUrl}
            fallback={props.displayName.charAt(0).toUpperCase()}
          >
            <img
              src={props.avatarUrl!}
              class="w-full h-full rounded-full object-cover"
            />
          </Show>
          <div class="absolute bottom-0 right-0 w-2.5 h-2.5 bg-green-500 border-2 border-surface-layer2 rounded-full" />
        </div>

        <div class="flex-1 min-w-0">
          <div class="text-sm font-semibold text-text-primary truncate">
            {props.displayName}
          </div>
        </div>
      </div>

      {/* Activity Content */}
      <div class="pl-2">
        <ActivityIndicator activity={props.activity} />
      </div>
    </div>
  );
};

export default ActiveActivityCard;
