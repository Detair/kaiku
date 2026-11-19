/**
 * Friends List Component
 *
 * Displays friends with tabs for Online, All, Pending, and Blocked.
 */

import { Component, createEffect, createSignal, createUniqueId, For, onMount, Show } from "solid-js";
import { Users, Search, UserPlus, MessageSquare } from "lucide-solid";
import { t, plural } from "@/i18n";
import type { MessageKey } from "@/i18n";
import { showToast } from "@/components/ui/Toast";
import { openDirectMessage } from "@/stores/dms";
import {
  friendsState,
  loadFriends,
  loadPendingRequests,
  loadBlocked,
  getOnlineFriends,
  getIncomingRequestCount,
  requestsTabWanted,
  takeRequestsTabWish,
  acceptFriendRequest,
  rejectFriendRequest,
  cancelFriendRequest,
  removeFriend,
  unblockUser,
} from "@/stores/friends";
import { getUserActivity, getUserStatus, getUserPresence } from "@/stores/presence";
import type { Friend } from "@/lib/types";
import { formatRelativeTime, truncate } from "@/lib/utils";
import { ActivityIndicator } from "@/components/ui";
import AddFriend from "./AddFriend";
import flokiHappy from "@/assets/emotes/floki_emote_1.webp";
import flokiThinking from "@/assets/emotes/floki_emote_2.webp";
import flokiCool from "@/assets/emotes/floki_emote_4.webp";

type FriendsTab = "online" | "all" | "pending" | "blocked";

const TAB_LABEL: Record<FriendsTab, MessageKey> = {
  online: "home.friends.tab.online",
  all: "home.friends.tab.all",
  pending: "home.friends.tab.pending",
  blocked: "home.friends.tab.blocked",
};

const EMPTY_TITLE: Record<FriendsTab, MessageKey> = {
  online: "home.friends.empty.onlineTitle",
  all: "home.friends.empty.allTitle",
  pending: "home.friends.empty.pendingTitle",
  blocked: "home.friends.empty.blockedTitle",
};

const EMPTY_HINT: Record<FriendsTab, MessageKey> = {
  online: "home.friends.empty.onlineHint",
  all: "home.friends.empty.allHint",
  pending: "home.friends.empty.pendingHint",
  blocked: "home.friends.empty.blockedHint",
};

const FriendsList: Component = () => {
  const [tab, setTab] = createSignal<FriendsTab>("online");
  const [showAddFriend, setShowAddFriend] = createSignal(false);
  const [filterQuery, setFilterQuery] = createSignal("");

  onMount(async () => {
    await Promise.all([loadFriends(), loadPendingRequests(), loadBlocked()]);
  });

  // "Show" on a new-request notice lands on the Requests tab (K3).
  createEffect(() => {
    if (requestsTabWanted()) {
      setTab("pending");
      takeRequestsTabWish();
    }
  });

  const filteredFriends = () => {
    let list: Friend[] = [];
    switch (tab()) {
      case "online":
        list = getOnlineFriends();
        break;
      case "all":
        list = friendsState.friends;
        break;
      case "pending":
        list = friendsState.pendingRequests;
        break;
      case "blocked":
        list = friendsState.blocked;
        break;
    }

    const query = filterQuery().toLowerCase();
    if (!query) return list;

    return list.filter(
      (f) =>
        f.display_name.toLowerCase().includes(query) ||
        f.username.toLowerCase().includes(query),
    );
  };

  const handleAccept = async (friendshipId: string) => {
    try {
      await acceptFriendRequest(friendshipId);
    } catch (err) {
      // Store already shows toast; just log here
      console.error("Failed to accept friend request:", err);
    }
  };

  const handleReject = async (friendshipId: string) => {
    try {
      await rejectFriendRequest(friendshipId);
    } catch (err) {
      // Store already shows toast; just log here
      console.error("Failed to reject friend request:", err);
    }
  };

  const handleCancel = async (friendshipId: string) => {
    try {
      await cancelFriendRequest(friendshipId);
    } catch (err) {
      // Store already shows toast; just log here
      console.error("Failed to cancel friend request:", err);
    }
  };

  const handleRemove = async (friendshipId: string) => {
    try {
      if (confirm(t("home.friends.removeConfirm"))) {
        await removeFriend(friendshipId);
      }
    } catch (err) {
      // Store already shows toast; just log here
      console.error("Failed to remove friend:", err);
    }
  };

  // NAV-3: a friend is someone you can write to.
  const handleMessage = async (userId: string) => {
    try {
      await openDirectMessage([userId]);
    } catch (err) {
      console.error("Failed to open conversation:", err);
      showToast({
        type: "error",
        title: t("home.friends.toast.messageFailedTitle"),
        message: t("home.friends.toast.messageFailed"),
        duration: 8000,
      });
    }
  };

  const handleUnblock = async (userId: string) => {
    try {
      await unblockUser(userId);
    } catch (err) {
      // Store already shows toast; just log here
      console.error("Failed to unblock user:", err);
    }
  };

  return (
    <div class="flex-1 flex flex-col h-full">
      {/* Tab bar with Search: wraps on narrow windows (NAV-13, LIVE-8) */}
      <div data-testid="friends-header" class="flex flex-wrap items-center gap-x-2 gap-y-2 px-4 py-3 border-b border-white/10">
        <div class="flex items-center gap-1 text-text-primary mr-2">
          <Users class="w-5 h-5" aria-hidden="true" />
          <h2 class="font-bold">{t("home.sidebar.friends")}</h2>
        </div>

        <div class="flex flex-wrap items-center gap-1">
        <button
          data-testid="friends-tab-online"
          onClick={() => setTab("online")}
          aria-pressed={tab() === "online"}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "online",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "online",
          }}
        >
          {t(TAB_LABEL.online)}
        </button>
        <button
          data-testid="friends-tab-all"
          onClick={() => setTab("all")}
          aria-pressed={tab() === "all"}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "all",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "all",
          }}
        >
          {t(TAB_LABEL.all)}
        </button>
        <button
          data-testid="friends-tab-pending"
          onClick={() => setTab("pending")}
          aria-pressed={tab() === "pending"}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "pending",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "pending",
          }}
        >
          {t(TAB_LABEL.pending)}
          <Show when={getIncomingRequestCount() > 0}>
            <span class="ml-2 px-1.5 py-0.5 bg-accent-primary text-on-accent text-xs font-bold rounded-full">
              {getIncomingRequestCount()}
            </span>
          </Show>
        </button>
        <button
          data-testid="friends-tab-blocked"
          onClick={() => setTab("blocked")}
          aria-pressed={tab() === "blocked"}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "blocked",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "blocked",
          }}
        >
          {t(TAB_LABEL.blocked)}
        </button>
        </div>

        <div class="flex-1" />

        <Show when={tab() !== "pending" && tab() !== "blocked"}>
          <div class="relative w-48 max-w-full">
            <input
              type="text"
              placeholder={t("home.friends.searchPlaceholder")}
              aria-label={t("home.friends.searchPlaceholder")}
              value={filterQuery()}
              onInput={(e) => setFilterQuery(e.currentTarget.value)}
              class="w-full pl-8 pr-3 py-1 bg-surface-base rounded-md text-sm text-text-input outline-none border border-white/5 focus:border-accent-primary/50 transition-colors"
            />
            <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-secondary" aria-hidden="true" />
          </div>
        </Show>

        <button
          data-testid="add-friend-button"
          onClick={() => setShowAddFriend(true)}
          class="btn-primary py-1 px-3 text-sm flex items-center gap-2 whitespace-nowrap"
        >
          <UserPlus class="w-4 h-4" aria-hidden="true" />
          {t("home.sidebar.addFriend")}
        </button>
      </div>

      {/* Requests waiting for me, at every window width (NAV-4, NAV-17) */}
      <Show when={tab() !== "pending" && getIncomingRequestCount() > 0}>
        <div
          data-testid="friends-request-banner"
          class="mx-4 mt-3 flex items-center gap-3 px-3 py-2 rounded-lg bg-accent-primary/20 border border-accent-primary/30"
        >
          <UserPlus class="w-4 h-4 text-text-primary flex-shrink-0" aria-hidden="true" />
          <span class="flex-1 text-sm text-text-primary">
            {plural(getIncomingRequestCount(), {
              one: "home.friends.requestBanner.one",
              other: "home.friends.requestBanner.other",
            })}
          </span>
          <button
            class="px-3 py-1 rounded-lg text-sm font-medium bg-white/10 text-text-primary hover:bg-white/20 transition-colors"
            onClick={() => setTab("pending")}
          >
            {t("home.friends.requestBannerShow")}
          </button>
        </div>
      </Show>

      {/* Friend list */}
      <div class="flex-1 overflow-y-auto">
        <Show
          when={!friendsState.isLoading && filteredFriends().length > 0}
          fallback={
            <div class="flex flex-col items-center justify-center h-full py-12">
              <Show
                when={!friendsState.isLoading}
                fallback={<div class="text-text-secondary">{t("common.loading")}</div>}
              >
                <img
                  src={
                    tab() === "blocked"
                      ? flokiCool
                      : tab() === "all"
                        ? flokiHappy
                        : flokiThinking
                  }
                  alt={""}
                  class="w-12 h-12 object-contain mb-3"
                  loading="lazy"
                />
                <div class="text-sm font-medium text-text-primary mb-1">
                  {t(EMPTY_TITLE[tab()])}
                </div>
                <p class="text-xs text-text-secondary text-center px-4">
                  {t(EMPTY_HINT[tab()])}
                </p>
                <Show when={tab() === "all" || tab() === "online"}>
                  <button
                    onClick={() => setShowAddFriend(true)}
                    class="mt-3 btn-primary py-1.5 px-4 text-sm flex items-center gap-2"
                  >
                    <UserPlus class="w-4 h-4" aria-hidden="true" />
                    {t("home.sidebar.addFriend")}
                  </button>
                </Show>
              </Show>
            </div>
          }
        >
          <div class="space-y-2 p-4">
            <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2 px-2">
              {t("home.friends.sectionCount", { tab: t(TAB_LABEL[tab()]), count: filteredFriends().length })}
            </div>
            <For each={filteredFriends()}>
              {(friend) => (
                <FriendItem
                  friend={friend}
                  tab={tab()}
                  onAccept={handleAccept}
                  onReject={handleReject}
                  onCancel={handleCancel}
                  onRemove={handleRemove}
                  onMessage={handleMessage}
                  onUnblock={handleUnblock}
                />
              )}
            </For>
          </div>
        </Show>
      </div>

      {/* Add Friend Modal */}
      <Show when={showAddFriend()}>
        <AddFriend onClose={() => setShowAddFriend(false)} />
      </Show>
    </div>
  );
};

interface FriendItemProps {
  friend: Friend;
  tab: FriendsTab;
  onAccept: (friendshipId: string) => void;
  onReject: (friendshipId: string) => void;
  onCancel: (friendshipId: string) => void;
  onRemove: (friendshipId: string) => void;
  onMessage: (userId: string) => void;
  onUnblock: (userId: string) => void;
}

const FriendItem: Component<FriendItemProps> = (props) => {
  const status = () => {
    const presenceStatus = getUserStatus(props.friend.user_id);
    if (presenceStatus !== "offline") {
      return presenceStatus;
    }

    return props.friend.is_online ? "online" : "offline";
  };

  const customStatusDisplay = () => {
    const customStatus = getUserPresence(props.friend.user_id)?.customStatus;
    if (customStatus?.text?.trim()) {
      return `${customStatus.emoji ? `${customStatus.emoji} ` : ""}${customStatus.text}`.trim();
    }

    return props.friend.status_message?.trim() || "";
  };

  const offlineLastSeenText = () => {
    const lastSeen = getUserPresence(props.friend.user_id)?.lastSeen || props.friend.last_seen;
    if (!lastSeen || status() !== "offline") {
      return null;
    }

    return t("home.friends.lastSeen", { time: formatRelativeTime(lastSeen) });
  };

  const canMessage = () => props.tab === "all" || props.tab === "online";

  const infoId = createUniqueId();

  return (
    <div data-testid="friend-item" class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors">
      {/* Avatar and name open the conversation for friends (NAV-3) */}
      <button
        type="button"
        data-testid="friend-open"
        class="flex items-center gap-3 flex-1 min-w-0 text-left rounded-lg disabled:cursor-default focus-visible:ring-2 focus-visible:ring-accent-primary/50 outline-none"
        disabled={!canMessage()}
        onClick={() => props.onMessage(props.friend.user_id)}
        aria-label={canMessage() ? t("home.friends.messageTo", { name: props.friend.display_name }) : undefined}
        aria-describedby={infoId}
      >
      {/* Avatar */}
      <div class="relative" aria-hidden="true">
        <div class="w-10 h-10 rounded-full bg-accent-primary flex items-center justify-center font-semibold text-on-accent">
          {props.friend.display_name.charAt(0).toUpperCase()}
        </div>
        <Show when={props.friend.is_online && props.tab !== "blocked"}>
          <div class="absolute bottom-0 right-0 w-3 h-3 bg-green-500 border-2 border-surface-base rounded-full" />
        </Show>
      </div>

      {/* Info: also the description of the button above (UX2 K12) */}
      <div id={infoId} class="flex-1 min-w-0">
        <Show when={props.friend.is_online && props.tab !== "blocked"}>
          <span class="sr-only">{t("home.status.online")}</span>
        </Show>
        <div class="font-semibold text-text-primary truncate">
          {props.friend.display_name}
        </div>
        <div class="flex items-center gap-2 min-w-0">
          <div class="text-sm text-text-secondary truncate">@{props.friend.username}</div>
          <Show when={customStatusDisplay().length > 0}>
            <div class="text-xs text-text-secondary truncate max-w-48">
              {truncate(customStatusDisplay(), 36)}
            </div>
          </Show>
        </div>
        <Show when={offlineLastSeenText()}>
          <div class="text-xs text-text-secondary/80 truncate">{offlineLastSeenText()}</div>
        </Show>
        {/* Activity indicator */}
        <Show when={getUserActivity(props.friend.user_id)}>
          <ActivityIndicator
            activity={getUserActivity(props.friend.user_id)!}
            compact
          />
        </Show>
      </div>
      </button>

      {/* Actions */}
      <div class="flex gap-2">
        <Show when={props.tab === "pending"}>
          <Show
            when={props.friend.direction === "outgoing"}
            fallback={
              <>
                <button
                  onClick={() => props.onAccept(props.friend.friendship_id)}
                  class="p-1.5 bg-white/5 rounded-lg hover:bg-status-success/20 transition-colors flex items-center justify-center w-9 h-9"
                  title={t("home.pending.accept")}
                  aria-label={t("home.pending.accept")}
                >
                  <div style={{ "background-image": "var(--icon-accept)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-80 hover:opacity-100" />
                </button>
                <button
                  onClick={() => props.onReject(props.friend.friendship_id)}
                  class="p-1.5 bg-white/5 rounded-lg hover:bg-status-error/20 transition-colors flex items-center justify-center w-9 h-9"
                  title={t("home.pending.decline")}
                  aria-label={t("home.pending.decline")}
                >
                  <div style={{ "background-image": "var(--icon-decline)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-80 hover:opacity-100" />
                </button>
              </>
            }
          >
            <button
              onClick={() => props.onCancel(props.friend.friendship_id)}
              class="px-3 py-1.5 bg-white/10 text-text-secondary rounded-lg text-sm font-medium hover:bg-status-error/20 hover:text-text-primary transition-colors"
              title={t("home.pending.cancelRequest")}
            >
              {t("home.pending.cancelRequest")}
            </button>
          </Show>
        </Show>
        <Show when={canMessage()}>
          <button
            data-testid="friend-message"
            onClick={() => props.onMessage(props.friend.user_id)}
            class="px-3 py-1.5 bg-accent-primary text-on-accent rounded-lg text-sm font-medium hover:opacity-90 transition-opacity flex items-center gap-1.5"
          >
            <MessageSquare class="w-4 h-4" aria-hidden="true" />
            {t("home.friends.message")}
          </button>
          {/* Quiet, not the loudest button on the page (NAV-3); asks first */}
          <button
            data-testid="friend-remove"
            onClick={() => props.onRemove(props.friend.friendship_id)}
            class="px-3 py-1.5 text-text-secondary rounded-lg text-sm hover:text-text-primary hover:bg-white/10 transition-colors"
            aria-label={t("home.friends.removeNamed", { name: props.friend.display_name })}
          >
            {t("home.friends.remove")}
          </button>
        </Show>
        <Show when={props.tab === "blocked"}>
          <button
            onClick={() => props.onUnblock(props.friend.user_id)}
            class="px-3 py-1.5 bg-white/10 text-text-primary rounded-lg text-sm font-medium hover:bg-white/20 transition-colors"
          >
            {t("home.friends.unblock")}
          </button>
        </Show>
      </div>
    </div>
  );
};

export default FriendsList;
