/**
 * GuildCard - Display card for a discoverable guild.
 */

import { Component, Show, For, createSignal } from "solid-js";
import { Users } from "lucide-solid";
import type { DiscoverableGuild } from "@/lib/types";
import { joinDiscoverable } from "@/lib/tauri";
import { loadGuilds } from "@/stores/guilds";
import { showToast } from "@/components/ui/Toast";

interface GuildCardProps {
  guild: DiscoverableGuild;
  isMember?: boolean;
}

const GuildCard: Component<GuildCardProps> = (props) => {
  const [joining, setJoining] = createSignal(false);
  const [localJoined, setLocalJoined] = createSignal(false);

  // Derive joined state reactively from prop + local state (Issue #19)
  const joined = () => localJoined() || (props.isMember ?? false);

  const initials = () =>
    props.guild.name
      .split(" ")
      .filter((w) => w.length > 0)
      .map((w) => w[0])
      .join("")
      .toUpperCase()
      .slice(0, 2);

  const truncatedDescription = () => {
    const desc = props.guild.description ?? "";
    return desc.length > 120 ? desc.slice(0, 117) + "..." : desc;
  };

  const handleJoin = async () => {
    if (joined() || joining()) return;
    setJoining(true);
    try {
      const result = await joinDiscoverable(props.guild.id);
      if (result.already_member) {
        setLocalJoined(true);
        showToast({ type: "info", title: "Already a Member", message: `You're already in ${result.guild_name}.` });
      } else {
        setLocalJoined(true);
        showToast({ type: "success", title: "Joined!", message: `You've joined ${result.guild_name}.` });
        // Refresh guild list so sidebar shows the new guild (stay in discovery view)
        try {
          await loadGuilds();
        } catch (refreshErr) {
          console.error("Failed to refresh guild list after join:", refreshErr);
        }
      }
    } catch (err) {
      console.error("Failed to join guild:", err);
      showToast({ type: "error", title: "Join Failed", message: "Could not join this server.", duration: 8000 });
    } finally {
      setJoining(false);
    }
  };

  return (
    <div class="flex flex-col rounded-xl border border-white/5 overflow-hidden bg-surface-layer2 hover:border-white/10 transition-colors">
      {/* Banner area */}
      <div class="h-24 relative">
        <Show
          when={props.guild.banner_url}
          fallback={
            <div
              class="w-full h-full"
              style={{
                background: `linear-gradient(135deg, var(--color-accent-primary) 0%, var(--color-surface-layer1) 100%)`,
                opacity: "0.6",
              }}
            />
          }
        >
          <img
            src={props.guild.banner_url!}
            alt=""
            class="w-full h-full object-cover"
          />
        </Show>
        {/* Guild icon overlapping the banner */}
        <div class="absolute -bottom-5 left-4">
          <div class="w-10 h-10 rounded-xl bg-surface-layer1 border-2 border-surface-layer2 flex items-center justify-center overflow-hidden">
            <Show
              when={props.guild.icon_url}
              fallback={
                <span class="text-xs font-bold text-text-primary">{initials()}</span>
              }
            >
              <img src={props.guild.icon_url!} alt="" class="w-full h-full object-cover" />
            </Show>
          </div>
        </div>
      </div>

      {/* Content */}
      <div class="pt-7 px-4 pb-4 flex flex-col flex-1">
        <h3 class="text-sm font-semibold text-text-primary truncate">{props.guild.name}</h3>

        <Show when={truncatedDescription()}>
          <p class="text-xs text-text-secondary mt-1 line-clamp-2">{truncatedDescription()}</p>
        </Show>

        {/* Tags (Issue #17: use <For> instead of .map) */}
        <Show when={props.guild.tags.length > 0}>
          <div class="flex flex-wrap gap-1 mt-2">
            <For each={props.guild.tags}>
              {(tag) => (
                <span class="px-1.5 py-0.5 text-[10px] rounded bg-white/5 text-text-secondary">
                  {tag}
                </span>
              )}
            </For>
          </div>
        </Show>

        <div class="flex-1" />

        {/* Footer */}
        <div class="flex items-center justify-between mt-3 pt-3 border-t border-white/5">
          <div class="flex items-center gap-1 text-xs text-text-secondary">
            <Users class="w-3 h-3" />
            <span>{props.guild.member_count.toLocaleString()}</span>
          </div>

          <button
            onClick={handleJoin}
            disabled={joined() || joining()}
            class="px-3 py-1 text-xs font-medium rounded-lg transition-colors"
            classList={{
              "bg-accent-primary text-white hover:bg-accent-hover": !joined(),
              "bg-white/10 text-text-secondary cursor-default": joined(),
            }}
          >
            {joined() ? "Joined" : joining() ? "Joining..." : "Join"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default GuildCard;
