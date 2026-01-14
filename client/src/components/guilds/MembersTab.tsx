/**
 * MembersTab - Member list with search and kick functionality
 */

import { Component, createSignal, createMemo, For, Show, onMount } from "solid-js";
import { Search, Crown, X } from "lucide-solid";
import { guildsState, loadGuildMembers, getGuildMembers, kickMember } from "@/stores/guilds";
import type { GuildMember } from "@/lib/types";

interface MembersTabProps {
  guildId: string;
  isOwner: boolean;
}

const MembersTab: Component<MembersTabProps> = (props) => {
  const [search, setSearch] = createSignal("");
  const [kickingId, setKickingId] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildMembers(props.guildId);
  });

  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const members = () => getGuildMembers(props.guildId);

  const filteredMembers = createMemo(() => {
    const query = search().toLowerCase().trim();
    if (!query) return members();
    return members().filter(
      (m) =>
        m.display_name.toLowerCase().includes(query) ||
        m.username.toLowerCase().includes(query)
    );
  });

  const handleKick = async (userId: string) => {
    if (kickingId() === userId) {
      // Confirmed, kick them
      try {
        await kickMember(props.guildId, userId);
      } catch (err) {
        console.error("Failed to kick member:", err);
      }
      setKickingId(null);
    } else {
      // First click, show confirmation
      setKickingId(userId);
      setTimeout(() => setKickingId(null), 3000);
    }
  };

  const formatLastSeen = (member: GuildMember): string => {
    if (member.status === "online") return "Online";
    if (member.status === "idle") return "Idle";
    if (!member.last_seen_at) return "Never";

    const lastSeen = new Date(member.last_seen_at);
    const now = new Date();
    const diff = now.getTime() - lastSeen.getTime();

    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 60) return `${minutes} min${minutes !== 1 ? "s" : ""} ago`;
    if (hours < 24) return `${hours} hour${hours !== 1 ? "s" : ""} ago`;
    if (days < 7) return `${days} day${days !== 1 ? "s" : ""} ago`;
    return lastSeen.toLocaleDateString();
  };

  const getStatusColor = (status: string): string => {
    switch (status) {
      case "online": return "#22c55e"; // green
      case "idle": return "#eab308"; // yellow
      default: return "#6b7280"; // gray
    }
  };

  const formatJoinDate = (date: string): string => {
    return new Date(date).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  };

  return (
    <div class="p-6">
      {/* Search */}
      <div class="relative mb-4">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
        <input
          type="text"
          value={search()}
          onInput={(e) => setSearch(e.currentTarget.value)}
          placeholder="Search members..."
          class="w-full pl-10 pr-4 py-2 rounded-lg border border-white/10 text-text-primary placeholder-text-secondary"
          style="background-color: var(--color-surface-layer1)"
        />
      </div>

      {/* Member Count */}
      <div class="text-sm text-text-secondary mb-3">
        {filteredMembers().length} member{filteredMembers().length !== 1 ? "s" : ""}
        {search() && ` matching "${search()}"`}
      </div>

      {/* Members List */}
      <Show
        when={filteredMembers().length > 0}
        fallback={
          <div class="text-center py-8 text-text-secondary">
            {search() ? "No members match your search" : "You're the only one here. Invite some friends!"}
          </div>
        }
      >
        <div class="space-y-1">
          <For each={filteredMembers()}>
            {(member) => {
              const isGuildOwner = member.user_id === guild()?.owner_id;
              const canKick = props.isOwner && !isGuildOwner;

              return (
                <div
                  class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors group"
                >
                  {/* Avatar with status indicator */}
                  <div class="relative">
                    <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                      <Show
                        when={member.avatar_url}
                        fallback={
                          <span class="text-sm font-semibold text-accent-primary">
                            {member.display_name.charAt(0).toUpperCase()}
                          </span>
                        }
                      >
                        <img
                          src={member.avatar_url!}
                          alt={member.display_name}
                          class="w-10 h-10 rounded-full object-cover"
                        />
                      </Show>
                    </div>
                    {/* Status dot */}
                    <div
                      class="absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 rounded-full border-2"
                      style={{
                        "background-color": getStatusColor(member.status),
                        "border-color": "var(--color-surface-base)",
                      }}
                    />
                  </div>

                  {/* Member info */}
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="font-medium text-text-primary truncate">
                        {member.nickname || member.display_name}
                      </span>
                      <Show when={isGuildOwner}>
                        <span title="Server Owner">
                          <Crown class="w-4 h-4 text-yellow-500" />
                        </span>
                      </Show>
                    </div>
                    <div class="text-sm text-text-secondary">
                      @{member.username}
                    </div>
                    <div class="text-xs text-text-secondary mt-0.5">
                      Joined {formatJoinDate(member.joined_at)} &bull; {formatLastSeen(member)}
                    </div>
                  </div>

                  {/* Kick button */}
                  <Show when={canKick}>
                    <button
                      onClick={() => handleKick(member.user_id)}
                      class="p-2 rounded-lg transition-all opacity-0 group-hover:opacity-100"
                      classList={{
                        "bg-accent-danger text-white": kickingId() === member.user_id,
                        "text-text-secondary hover:text-accent-danger hover:bg-white/10": kickingId() !== member.user_id,
                      }}
                      title={kickingId() === member.user_id ? "Click to confirm" : "Kick member"}
                    >
                      <Show
                        when={kickingId() === member.user_id}
                        fallback={<X class="w-4 h-4" />}
                      >
                        <span class="text-xs px-1">Confirm?</span>
                      </Show>
                    </button>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>

      {/* Loading state */}
      <Show when={guildsState.isMembersLoading}>
        <div class="text-center py-4 text-text-secondary">Loading members...</div>
      </Show>
    </div>
  );
};

export default MembersTab;
