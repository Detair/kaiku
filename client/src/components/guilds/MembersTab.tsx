/**
 * MembersTab - Member list with search, role badges, and role management
 */

import {
  Component,
  createSignal,
  createMemo,
  For,
  Show,
  onMount,
} from "solid-js";
import { createVirtualizer } from "@/lib/virtualizer";
import { Search, Crown } from "lucide-solid";
import {
  guildsState,
  loadGuildMembers,
  getGuildMembers,
} from "@/stores/guilds";
import {
  loadGuildRoles,
  loadMemberRoles,
  getMemberRoles,
  memberHasPermission,
  canModerateMember,
} from "@/stores/permissions";
import { getUserActivity } from "@/stores/presence";
import { PermissionBits } from "@/lib/permissionConstants";
import { authState } from "@/stores/auth";
import MemberRoleDropdown from "./MemberRoleDropdown";
import { ActivityIndicator } from "../ui";
import type { GuildMember } from "@/lib/types";
import { showUserContextMenu } from "@/lib/contextMenuBuilders";

interface MembersTabProps {
  guildId: string;
  isOwner: boolean;
}

const MembersTab: Component<MembersTabProps> = (props) => {
  const [search, setSearch] = createSignal("");

  onMount(() => {
    loadGuildMembers(props.guildId);
    loadGuildRoles(props.guildId);
    loadMemberRoles(props.guildId);
  });

  // Check if current user can manage roles
  const canManageRoles = () =>
    props.isOwner ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      props.isOwner,
      PermissionBits.MANAGE_ROLES,
    );

  const canModerate = (memberUserId: string): boolean => {
    const currentUserId = authState.user?.id;
    if (!currentUserId) return false;

    return canModerateMember(
      props.guildId,
      currentUserId,
      memberUserId,
      props.isOwner,
      PermissionBits.KICK_MEMBERS,
    );
  };

  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const members = () => getGuildMembers(props.guildId);

  const filteredMembers = createMemo(() => {
    const query = search().toLowerCase().trim();
    if (!query) return members();
    return members().filter(
      (m) =>
        m.display_name.toLowerCase().includes(query) ||
        m.username.toLowerCase().includes(query),
    );
  });

  let membersContainerRef: HTMLDivElement | undefined;

  const virtualizer = createVirtualizer({
    get count() {
      return filteredMembers().length;
    },
    getScrollElement: () => membersContainerRef ?? null,
    estimateSize: () => 80,
    overscan: 5,
  });

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
      case "online":
        return "#22c55e"; // green
      case "idle":
        return "#eab308"; // yellow
      default:
        return "#6b7280"; // gray
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
    <div class="p-6 flex flex-col h-full">
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
        {filteredMembers().length} member
        {filteredMembers().length !== 1 ? "s" : ""}
        {search() && ` matching "${search()}"`}
      </div>

      {/* Members List */}
      <div ref={membersContainerRef} class="flex-1 overflow-y-auto min-h-0">
        <Show
          when={filteredMembers().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary">
              {search()
                ? "No members match your search"
                : "You're the only one here. Invite some friends!"}
            </div>
          }
        >
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              position: "relative",
            }}
          >
            <For each={virtualizer.getVirtualItems()}>
              {(virtualItem) => {
                const member = () => filteredMembers()[virtualItem.index];
                return (
                  <div
                    data-index={virtualItem.index}
                    ref={(el) => virtualizer.measureElement(el)}
                    style={{
                      position: "absolute",
                      top: `${virtualItem.start}px`,
                      width: "100%",
                    }}
                  >
                    <Show when={member()}>
                      {(m) => {
                        const isMemberOwner = () =>
                          m().user_id === guild()?.owner_id;
                        const memberRoles = () =>
                          getMemberRoles(props.guildId, m().user_id);

                        return (
                          <div
                            class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors group"
                            onContextMenu={(e) =>
                              showUserContextMenu(e, {
                                id: m().user_id,
                                username: m().username,
                                display_name: m().display_name,
                              })
                            }
                          >
                            {/* Avatar with status indicator */}
                            <div class="relative flex-shrink-0">
                              <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                                <Show
                                  when={m().avatar_url}
                                  fallback={
                                    <span class="text-sm font-semibold text-accent-primary">
                                      {m().display_name.charAt(0).toUpperCase()}
                                    </span>
                                  }
                                >
                                  <img
                                    src={m().avatar_url!}
                                    alt={m().display_name}
                                    class="w-10 h-10 rounded-full object-cover"
                                  />
                                </Show>
                              </div>
                              {/* Status dot */}
                              <div
                                class="absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 rounded-full border-2"
                                style={{
                                  "background-color": getStatusColor(
                                    m().status,
                                  ),
                                  "border-color": "var(--color-surface-base)",
                                }}
                              />
                            </div>

                            {/* Member info */}
                            <div class="flex-1 min-w-0">
                              <div class="flex items-center gap-2">
                                <span class="font-medium text-text-primary truncate">
                                  {m().nickname || m().display_name}
                                </span>
                                <Show when={isMemberOwner()}>
                                  <span title="Server Owner">
                                    <Crown class="w-4 h-4 text-yellow-500" />
                                  </span>
                                </Show>
                              </div>
                              <div class="text-sm text-text-secondary">
                                @{m().username}
                              </div>
                              <div class="text-xs text-text-secondary mt-0.5">
                                Joined {formatJoinDate(m().joined_at)} &bull;{" "}
                                {formatLastSeen(m())}
                              </div>
                              {/* Activity indicator */}
                              <Show when={getUserActivity(m().user_id)}>
                                <ActivityIndicator
                                  activity={getUserActivity(m().user_id)!}
                                  compact
                                />
                              </Show>
                            </div>

                            {/* Role badges */}
                            <div class="flex items-center gap-1 flex-wrap flex-shrink-0">
                              <For each={memberRoles()}>
                                {(role) => (
                                  <span
                                    class="px-1.5 py-0.5 text-xs rounded-full"
                                    style={{
                                      "background-color": role.color
                                        ? `${role.color}20`
                                        : "var(--color-surface-layer1)",
                                      color:
                                        role.color ||
                                        "var(--color-text-secondary)",
                                      border: `1px solid ${role.color || "var(--color-white-10)"}`,
                                    }}
                                  >
                                    {role.name}
                                  </span>
                                )}
                              </For>
                              <Show when={memberRoles().length === 0}>
                                <span class="text-xs text-text-secondary">
                                  (no roles)
                                </span>
                              </Show>
                            </div>

                            {/* Manage dropdown - replaces kick button */}
                            <Show
                              when={
                                !isMemberOwner() &&
                                (canManageRoles() || canModerate(m().user_id))
                              }
                            >
                              <MemberRoleDropdown
                                guildId={props.guildId}
                                userId={m().user_id}
                              />
                            </Show>
                          </div>
                        );
                      }}
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>
        </Show>
      </div>

      {/* Loading state */}
      <Show when={guildsState.isMembersLoading}>
        <div class="text-center py-4 text-text-secondary">
          Loading members...
        </div>
      </Show>
    </div>
  );
};

export default MembersTab;
