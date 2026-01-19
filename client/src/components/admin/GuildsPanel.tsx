/**
 * GuildsPanel - Guild management panel for admin dashboard
 *
 * Provides guild listing with search, pagination, and suspend/unsuspend functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import { Component, Show, For, onMount, createSignal, createMemo } from "solid-js";
import {
  Search,
  Building2,
  Ban,
  CheckCircle,
  ChevronLeft,
  ChevronRight,
  Users,
  X,
} from "lucide-solid";
import {
  adminState,
  loadGuilds,
  selectGuild,
  suspendGuild,
  unsuspendGuild,
} from "@/stores/admin";

const PAGE_SIZE = 20;

const GuildsPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [suspendReason, setSuspendReason] = createSignal("");
  const [showSuspendDialog, setShowSuspendDialog] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal(false);

  // Load guilds on mount
  onMount(() => {
    loadGuilds(1);
  });

  // Calculate total pages
  const totalPages = createMemo(() =>
    Math.ceil(adminState.guildsPagination.total / PAGE_SIZE) || 1
  );

  // Get currently selected guild
  const selectedGuild = createMemo(() =>
    adminState.guilds.find((g) => g.id === adminState.selectedGuildId) ?? null
  );

  // Filter guilds by search query
  const filteredGuilds = createMemo(() => {
    const query = searchQuery().toLowerCase().trim();
    if (!query) return adminState.guilds;
    return adminState.guilds.filter((g) => g.name.toLowerCase().includes(query));
  });

  // Handle page navigation
  const goToPage = (page: number) => {
    if (page >= 1 && page <= totalPages()) {
      loadGuilds(page);
    }
  };

  // Handle suspend action
  const handleSuspend = async () => {
    const guild = selectedGuild();
    if (!guild || !suspendReason().trim()) return;

    setActionLoading(true);
    try {
      await suspendGuild(guild.id, suspendReason());
      setShowSuspendDialog(false);
      setSuspendReason("");
    } finally {
      setActionLoading(false);
    }
  };

  // Handle unsuspend action
  const handleUnsuspend = async () => {
    const guild = selectedGuild();
    if (!guild) return;

    setActionLoading(true);
    try {
      await unsuspendGuild(guild.id);
    } finally {
      setActionLoading(false);
    }
  };

  // Format date for display
  const formatDate = (dateStr: string): string => {
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  };

  // Truncate ID for display
  const truncateId = (id: string): string => {
    return id.slice(0, 8) + "...";
  };

  return (
    <div class="flex flex-1 h-full overflow-hidden">
      {/* Guild List */}
      <div class="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 class="text-lg font-bold text-text-primary">Guilds</h2>

          {/* Search Input */}
          <div class="relative">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
            <input
              type="text"
              placeholder="Search guilds..."
              value={searchQuery()}
              onInput={(e) => setSearchQuery(e.currentTarget.value)}
              class="pl-9 pr-4 py-2 w-64 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm"
            />
          </div>
        </div>

        {/* Guild Table */}
        <div class="flex-1 overflow-auto">
          {/* Table Header */}
          <div class="grid grid-cols-4 gap-4 px-4 py-3 border-b border-white/10 bg-white/5 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0">
            <div>Name</div>
            <div>Members</div>
            <div>Created</div>
            <div>Status</div>
          </div>

          {/* Loading State */}
          <Show when={adminState.isGuildsLoading}>
            <div class="flex items-center justify-center py-12">
              <div class="text-text-secondary">Loading guilds...</div>
            </div>
          </Show>

          {/* Guild Rows */}
          <Show when={!adminState.isGuildsLoading}>
            <For
              each={filteredGuilds()}
              fallback={
                <div class="flex items-center justify-center py-12">
                  <div class="text-text-secondary">No guilds found</div>
                </div>
              }
            >
              {(guild) => (
                <div
                  onClick={() => selectGuild(guild.id)}
                  class="grid grid-cols-4 gap-4 px-4 py-3 border-b border-white/5 cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/10": adminState.selectedGuildId === guild.id,
                    "hover:bg-white/5": adminState.selectedGuildId !== guild.id,
                  }}
                >
                  {/* Name */}
                  <div class="flex items-center gap-3 min-w-0">
                    <div class="w-8 h-8 rounded-lg bg-accent-primary/20 flex items-center justify-center flex-shrink-0">
                      <Building2 class="w-4 h-4 text-accent-primary" />
                    </div>
                    <div class="text-sm font-medium text-text-primary truncate">
                      {guild.name}
                    </div>
                  </div>

                  {/* Members */}
                  <div class="flex items-center gap-2 text-sm text-text-secondary">
                    <Users class="w-4 h-4" />
                    {guild.member_count}
                  </div>

                  {/* Created */}
                  <div class="flex items-center text-sm text-text-secondary">
                    {formatDate(guild.created_at)}
                  </div>

                  {/* Status */}
                  <div class="flex items-center">
                    <Show
                      when={guild.suspended_at}
                      fallback={
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-status-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-status-error">
                        <Ban class="w-3 h-3" />
                        Suspended
                      </span>
                    </Show>
                  </div>
                </div>
              )}
            </For>
          </Show>
        </div>

        {/* Pagination */}
        <div class="flex items-center justify-between px-4 py-3 border-t border-white/10">
          <div class="text-sm text-text-secondary">
            {adminState.guildsPagination.total} total guilds
          </div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => goToPage(adminState.guildsPagination.page - 1)}
              disabled={adminState.guildsPagination.page <= 1}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-sm text-text-primary">
              Page {adminState.guildsPagination.page} of {totalPages()}
            </span>
            <button
              onClick={() => goToPage(adminState.guildsPagination.page + 1)}
              disabled={adminState.guildsPagination.page >= totalPages()}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronRight class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>

      {/* Detail Panel */}
      <Show when={selectedGuild()}>
        {(guild) => (
          <div class="w-80 flex-shrink-0 border-l border-white/10 flex flex-col">
            {/* Detail Header */}
            <div class="flex items-center justify-between p-4 border-b border-white/10">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                Guild Details
              </h3>
              <button
                onClick={() => selectGuild(null)}
                class="p-1 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
              >
                <X class="w-4 h-4" />
              </button>
            </div>

            {/* Detail Content */}
            <div class="flex-1 p-4 space-y-6 overflow-auto">
              {/* Profile Section */}
              <div class="flex flex-col items-center text-center space-y-3">
                <div class="w-16 h-16 rounded-xl bg-accent-primary/20 flex items-center justify-center">
                  <Building2 class="w-8 h-8 text-accent-primary" />
                </div>
                <div class="text-lg font-bold text-text-primary">
                  {guild().name}
                </div>
              </div>

              {/* Info Section */}
              <div class="space-y-4">
                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Members
                  </div>
                  <div class="flex items-center gap-2 text-sm text-text-primary">
                    <Users class="w-4 h-4 text-text-secondary" />
                    {guild().member_count}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Owner ID
                  </div>
                  <div class="text-sm text-text-primary font-mono">
                    {truncateId(guild().owner_id)}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Created
                  </div>
                  <div class="text-sm text-text-primary">
                    {formatDate(guild().created_at)}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Status
                  </div>
                  <div>
                    <Show
                      when={guild().suspended_at}
                      fallback={
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-status-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-status-error">
                        <Ban class="w-3 h-3" />
                        Suspended
                      </span>
                    </Show>
                  </div>
                </div>

                <Show when={guild().suspended_at}>
                  <div class="space-y-1">
                    <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                      Suspended Date
                    </div>
                    <div class="text-sm text-text-primary">
                      {formatDate(guild().suspended_at!)}
                    </div>
                  </div>
                </Show>
              </div>

              {/* Actions Section */}
              <div class="space-y-3 pt-4 border-t border-white/10">
                <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  Actions
                </div>

                <Show when={!adminState.isElevated}>
                  <div class="p-3 rounded-lg bg-status-warning/10 border border-status-warning/30 text-status-warning text-xs">
                    Requires elevation to perform actions
                  </div>
                </Show>

                <Show
                  when={guild().suspended_at}
                  fallback={
                    <button
                      onClick={() => setShowSuspendDialog(true)}
                      disabled={!adminState.isElevated || actionLoading()}
                      class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      <Ban class="w-4 h-4" />
                      Suspend Guild
                    </button>
                  }
                >
                  <button
                    onClick={handleUnsuspend}
                    disabled={!adminState.isElevated || actionLoading()}
                    class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-status-success text-white font-medium transition-colors hover:bg-status-success/90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <CheckCircle class="w-4 h-4" />
                    {actionLoading() ? "Processing..." : "Unsuspend Guild"}
                  </button>
                </Show>
              </div>
            </div>
          </div>
        )}
      </Show>

      {/* Suspend Dialog */}
      <Show when={showSuspendDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setShowSuspendDialog(false)}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-text-primary">
                Suspend Guild
              </h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to suspend{" "}
                <span class="font-medium text-text-primary">
                  {selectedGuild()?.name}
                </span>
                ? All members will be unable to access this guild.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Reason for suspension
                </label>
                <textarea
                  value={suspendReason()}
                  onInput={(e) => setSuspendReason(e.currentTarget.value)}
                  placeholder="Enter reason..."
                  rows={3}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm resize-none"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowSuspendDialog(false);
                    setSuspendReason("");
                  }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleSuspend}
                  disabled={!suspendReason().trim() || actionLoading()}
                  class="flex-1 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {actionLoading() ? "Suspending..." : "Confirm Suspend"}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default GuildsPanel;
