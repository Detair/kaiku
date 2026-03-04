/**
 * UnreadModule Component
 *
 * Shows aggregate unread counts across all guilds and DMs.
 * Provides quick navigation to channels with unread messages.
 */

import {
  Component,
  Show,
  For,
  createSignal,
  onMount,
  onCleanup,
} from "solid-js";
import { Inbox, Hash, CheckCheck } from "lucide-solid";
import {
  getUnreadAggregate,
  markAllGuildChannelsRead,
  markAllDMsRead,
  markAllRead,
  type UnreadAggregate,
} from "@/lib/tauri";
import { selectGuild } from "@/stores/guilds";
import { selectChannel, markAllGuildChannelsAsRead } from "@/stores/channels";
import { selectDM, markAllDMsAsRead } from "@/stores/dms";
import { showToast } from "@/components/ui/Toast";
import Skeleton from "@/components/ui/Skeleton";
import CollapsibleModule from "./CollapsibleModule";

const UnreadModule: Component = () => {
  const [unreadData, setUnreadData] = createSignal<UnreadAggregate | null>(
    null,
  );
  const [loading, setLoading] = createSignal(true);
  let inFlightFetch: Promise<void> | null = null;
  let nextAllowedFetchAt = 0;
  let cooldownTimer: ReturnType<typeof setTimeout> | undefined;
  let lastRateLimitToastAt = 0;

  const parseRetryAfterMs = (error: unknown): number | null => {
    const message = error instanceof Error ? error.message : String(error ?? "");
    const match = message.match(/wait\s+(\d+)\s+seconds?/i);
    if (!match) return null;

    const seconds = Number.parseInt(match[1], 10);
    if (!Number.isFinite(seconds) || seconds <= 0) return null;
    return seconds * 1000;
  };

  const scheduleCooldownRetry = (delayMs: number) => {
    if (cooldownTimer) {
      clearTimeout(cooldownTimer);
    }

    cooldownTimer = setTimeout(() => {
      if (Date.now() >= nextAllowedFetchAt) {
        void fetchUnreads();
      }
    }, delayMs + 150);
  };

  // Fetch unread data
  const fetchUnreads = async () => {
    if (Date.now() < nextAllowedFetchAt) {
      return;
    }

    if (inFlightFetch) {
      return inFlightFetch;
    }

    inFlightFetch = (async () => {
    try {
      const data = await getUnreadAggregate();
      setUnreadData(data);
      nextAllowedFetchAt = 0;
      lastFetchTime = Date.now();
    } catch (error) {
      const retryAfterMs = parseRetryAfterMs(error);
      if (retryAfterMs) {
        nextAllowedFetchAt = Date.now() + retryAfterMs;
        scheduleCooldownRetry(retryAfterMs);

        const now = Date.now();
        if (now - lastRateLimitToastAt > 15_000) {
          lastRateLimitToastAt = now;
          showToast({
            type: "warning",
            title: "Unread Refresh Rate-Limited",
            message: `Retrying in ${Math.ceil(retryAfterMs / 1000)} seconds.`,
            duration: 5000,
          });
        }

        return;
      }

      console.error("Failed to fetch unread aggregate:", error);
      showToast({
        type: "error",
        title: "Failed to Load Unreads",
        message:
          "Could not fetch unread messages. Will retry when window gains focus.",
        duration: 8000,
      });
    } finally {
      setLoading(false);
      inFlightFetch = null;
    }
    })();

    return inFlightFetch;
  };

  // Refresh when window gains focus (throttled to once per 30s)
  let lastFetchTime = 0;
  const FOCUS_REFRESH_INTERVAL_MS = 30_000;

  const throttledFetch = () => {
    const now = Date.now();
    if (now < nextAllowedFetchAt) {
      return;
    }
    if (now - lastFetchTime >= FOCUS_REFRESH_INTERVAL_MS) {
      void fetchUnreads();
    }
  };

  // Debounced fetch for WebSocket events (5s)
  let wsDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  const WS_DEBOUNCE_MS = 5_000;

  const debouncedFetch = () => {
    if (wsDebounceTimer) clearTimeout(wsDebounceTimer);
    wsDebounceTimer = setTimeout(() => {
      if (Date.now() >= nextAllowedFetchAt) {
        void fetchUnreads();
      }
    }, WS_DEBOUNCE_MS);
  };

  onMount(() => {
    lastFetchTime = Date.now();
    const handleFocus = () => throttledFetch();
    const handleUnreadUpdate = () => debouncedFetch();
    window.addEventListener("focus", handleFocus);
    window.addEventListener("unread-update", handleUnreadUpdate);

    void fetchUnreads();

    onCleanup(() => {
      window.removeEventListener("focus", handleFocus);
      window.removeEventListener("unread-update", handleUnreadUpdate);
    });
  });

  onCleanup(() => {
    if (wsDebounceTimer) clearTimeout(wsDebounceTimer);
    if (cooldownTimer) clearTimeout(cooldownTimer);
  });

  const totalUnread = () => unreadData()?.total ?? 0;
  const hasUnreads = () => totalUnread() > 0;

  const navigateToChannel = async (guildId: string, channelId: string) => {
    try {
      // Switch to guild first
      await selectGuild(guildId);
      // Then switch to channel
      selectChannel(channelId);
    } catch (error) {
      console.error("Failed to navigate to channel:", error);
      showToast({
        type: "error",
        title: "Navigation Failed",
        message: "Could not navigate to channel. Please try again.",
        duration: 8000,
      });
    }
  };

  const handleMarkAllRead = async () => {
    try {
      await markAllRead();
      setUnreadData(null);
    } catch (error) {
      console.error("Failed to mark all as read:", error);
      showToast({
        type: "error",
        title: "Mark All Read Failed",
        message: "Could not mark all as read.",
        duration: 8000,
      });
    }
  };

  const handleMarkGuildRead = async (guildId: string) => {
    try {
      await markAllGuildChannelsRead(guildId);
      markAllGuildChannelsAsRead(guildId);
      // Remove this guild from unread data
      const current = unreadData();
      if (current) {
        const guilds = current.guilds.filter((g) => g.guild_id !== guildId);
        const removedTotal =
          current.guilds.find((g) => g.guild_id === guildId)?.total_unread ?? 0;
        setUnreadData({
          ...current,
          guilds,
          total: current.total - removedTotal,
        });
      }
    } catch (error) {
      console.error("Failed to mark guild as read:", error);
      showToast({
        type: "error",
        title: "Mark All Read Failed",
        message: "Could not mark guild channels as read.",
        duration: 8000,
      });
    }
  };

  const handleMarkDMsRead = async () => {
    try {
      await markAllDMsRead();
      markAllDMsAsRead();
      // Remove DMs from unread data
      const current = unreadData();
      if (current) {
        const dmTotal = current.dms.reduce((sum, d) => sum + d.unread_count, 0);
        setUnreadData({ ...current, dms: [], total: current.total - dmTotal });
      }
    } catch (error) {
      console.error("Failed to mark DMs as read:", error);
      showToast({
        type: "error",
        title: "Mark All Read Failed",
        message: "Could not mark DMs as read.",
        duration: 8000,
      });
    }
  };

  const navigateToDM = (channelId: string) => {
    try {
      // Select the DM (this automatically switches to DMs view)
      selectDM(channelId);
    } catch (error) {
      console.error("Failed to navigate to DM:", error);
      showToast({
        type: "error",
        title: "Navigation Failed",
        message: "Could not navigate to DM. Please try again.",
        duration: 8000,
      });
    }
  };

  return (
    <CollapsibleModule id="unread" title="Unread" badge={totalUnread()}>
      <Show when={hasUnreads()}>
        <div class="flex justify-end mb-2">
          <button
            onClick={handleMarkAllRead}
            class="flex items-center gap-1 text-xs text-text-secondary hover:text-accent-primary transition-colors"
            title="Mark all as read"
          >
            <CheckCheck class="w-3.5 h-3.5" />
            Mark All as Read
          </button>
        </div>
      </Show>
      <Show
        when={!loading() && hasUnreads()}
        fallback={
          <Show
            when={!loading()}
            fallback={
              <div class="space-y-4">
                {/* Guild skeleton */}
                <div class="space-y-1">
                  <Skeleton width="80px" height="10px" />
                  <div class="space-y-1">
                    <div class="flex items-center justify-between px-2 py-1.5">
                      <div class="flex items-center gap-2">
                        <Skeleton width="16px" height="16px" />
                        <Skeleton width="120px" height="14px" />
                      </div>
                      <Skeleton width="24px" height="18px" />
                    </div>
                    <div class="flex items-center justify-between px-2 py-1.5">
                      <div class="flex items-center gap-2">
                        <Skeleton width="16px" height="16px" />
                        <Skeleton width="90px" height="14px" />
                      </div>
                      <Skeleton width="24px" height="18px" />
                    </div>
                  </div>
                </div>
                {/* DM skeleton */}
                <div class="space-y-1">
                  <Skeleton width="120px" height="10px" />
                  <div class="flex items-center justify-between px-2 py-1.5">
                    <Skeleton width="100px" height="14px" />
                    <Skeleton width="24px" height="18px" />
                  </div>
                </div>
              </div>
            }
          >
            <div class="flex flex-col items-center justify-center py-4 text-center">
              <Inbox class="w-8 h-8 text-text-secondary mb-2 opacity-50" />
              <p class="text-sm text-text-secondary">All caught up!</p>
              <p class="text-xs text-text-muted mt-1">No unread messages</p>
            </div>
          </Show>
        }
      >
        <div class="space-y-4">
          {/* Guild Unreads */}
          <Show when={unreadData()?.guilds && unreadData()!.guilds.length > 0}>
            <div class="space-y-3">
              <For each={unreadData()!.guilds}>
                {(guild) => (
                  <div class="space-y-1">
                    <div class="flex items-center justify-between">
                      <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
                        {guild.guild_name}
                      </div>
                      <button
                        onClick={() => handleMarkGuildRead(guild.guild_id)}
                        class="text-text-muted hover:text-accent-primary transition-colors"
                        title={`Mark all in ${guild.guild_name} as read`}
                      >
                        <CheckCheck class="w-3.5 h-3.5" />
                      </button>
                    </div>
                    <div class="space-y-1">
                      <For each={guild.channels}>
                        {(channel) => (
                          <button
                            onClick={() =>
                              navigateToChannel(
                                guild.guild_id,
                                channel.channel_id,
                              )
                            }
                            class="w-full flex items-center justify-between px-2 py-1.5 rounded hover:bg-white/5 transition-colors group"
                          >
                            <div class="flex items-center gap-2 min-w-0">
                              <Hash class="w-4 h-4 text-text-secondary flex-shrink-0" />
                              <span class="text-sm text-text-primary truncate group-hover:text-accent-primary">
                                {channel.channel_name}
                              </span>
                            </div>
                            <span class="px-1.5 py-0.5 text-xs font-medium bg-accent-primary text-white rounded flex-shrink-0">
                              {channel.unread_count}
                            </span>
                          </button>
                        )}
                      </For>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>

          {/* DM Unreads */}
          <Show when={unreadData()?.dms && unreadData()!.dms.length > 0}>
            <div class="space-y-1">
              <div class="flex items-center justify-between">
                <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
                  Direct Messages
                </div>
                <button
                  onClick={handleMarkDMsRead}
                  class="text-text-muted hover:text-accent-primary transition-colors"
                  title="Mark all DMs as read"
                >
                  <CheckCheck class="w-3.5 h-3.5" />
                </button>
              </div>
              <div class="space-y-1">
                <For each={unreadData()!.dms}>
                  {(dm) => (
                    <button
                      onClick={() => navigateToDM(dm.channel_id)}
                      class="w-full flex items-center justify-between px-2 py-1.5 rounded hover:bg-white/5 transition-colors group"
                    >
                      <span class="text-sm text-text-primary truncate group-hover:text-accent-primary">
                        {dm.channel_name}
                      </span>
                      <span class="px-1.5 py-0.5 text-xs font-medium bg-accent-primary text-white rounded flex-shrink-0">
                        {dm.unread_count}
                      </span>
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>
      </Show>
    </CollapsibleModule>
  );
};

export default UnreadModule;
