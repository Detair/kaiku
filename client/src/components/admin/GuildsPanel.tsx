/**
 * GuildsPanel - Guild management panel for admin dashboard
 *
 * Provides guild listing with search, pagination, and suspend/unsuspend functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import {
  Component,
  Show,
  For,
  onMount,
  createSignal,
  createMemo,
  onCleanup,
  createEffect,
} from "solid-js";
import {
  Search,
  Ban,
  CheckCircle,
  ChevronLeft,
  ChevronRight,
  Users,
  X,
  Crown,
  Loader2,
  Download,
  Square,
  CheckSquare,
  Trash2,
} from "lucide-solid";
import {
  adminState,
  loadGuilds,
  selectGuild,
  loadGuildDetails,
  suspendGuild,
  unsuspendGuild,
  deleteGuild,
  searchGuilds,
  toggleGuildSelection,
  selectAllGuilds,
  clearGuildSelection,
  isGuildSelected,
  getSelectedGuildCount,
  exportGuildsCsv,
  bulkSuspendGuilds,
} from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";
import TableRowSkeleton from "./TableRowSkeleton";
import { showToast } from "@/components/ui/Toast";

const PAGE_SIZE = 20;
const DEBOUNCE_MS = 300;

const GuildsPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [suspendReason, setSuspendReason] = createSignal("");
  const [showSuspendDialog, setShowSuspendDialog] = createSignal(false);
  const [showBulkSuspendDialog, setShowBulkSuspendDialog] = createSignal(false);
  const [bulkSuspendReason, setBulkSuspendReason] = createSignal("");
  const [showDeleteDialog, setShowDeleteDialog] = createSignal(false);
  const [deleteConfirmText, setDeleteConfirmText] = createSignal("");
  const [actionLoading, setActionLoading] = createSignal(false);
  const [focusedIndex, setFocusedIndex] = createSignal(-1);

  let listRef: HTMLDivElement | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Check if all guilds on current page are selected
  const allSelected = createMemo(() => {
    const guilds = adminState.guilds;
    return guilds.length > 0 && guilds.every((g) => isGuildSelected(g.id));
  });

  // Handle export
  const handleExport = async () => {
    await exportGuildsCsv();
  };

  // Handle bulk suspend
  const handleBulkSuspend = async () => {
    if (!bulkSuspendReason().trim()) return;

    setActionLoading(true);
    try {
      const result = await bulkSuspendGuilds(bulkSuspendReason());
      if (result) {
        setShowBulkSuspendDialog(false);
        setBulkSuspendReason("");
      }
    } finally {
      setActionLoading(false);
    }
  };

  // Load guilds on mount
  onMount(() => {
    loadGuilds(1);
  });

  // Cleanup debounce timer
  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
  });

  // Handle search input with debounce
  const handleSearchInput = (value: string) => {
    setSearchQuery(value);

    // Clear previous timer
    if (debounceTimer) clearTimeout(debounceTimer);

    // Set new timer for debounced search
    debounceTimer = setTimeout(() => {
      searchGuilds(value);
    }, DEBOUNCE_MS);
  };

  // Calculate total pages
  const totalPages = createMemo(
    () => Math.ceil(adminState.guildsPagination.total / PAGE_SIZE) || 1,
  );

  // Get currently selected guild
  const selectedGuild = createMemo(
    () =>
      adminState.guilds.find((g) => g.id === adminState.selectedGuildId) ??
      null,
  );

  // Load guild details when a guild is selected
  createEffect(() => {
    const guildId = adminState.selectedGuildId;
    if (guildId) {
      loadGuildDetails(guildId);
    }
  });

  // Guilds are now filtered server-side
  const filteredGuilds = createMemo(() => adminState.guilds);

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

      // Show undo toast
      const guildName = guild.name;
      const guildId = guild.id;
      showToast({
        id: `suspend-undo-${guildId}`,
        type: "success",
        title: "Guild suspended",
        message: `${guildName} has been suspended`,
        duration: 5000,
        action: {
          label: "Undo",
          onClick: async () => {
            await unsuspendGuild(guildId);
            showToast({
              type: "info",
              title: "Suspension reversed",
              message: `${guildName} has been unsuspended`,
              duration: 3000,
            });
          },
        },
      });
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
      showToast({
        type: "success",
        title: "Guild unsuspended",
        message: `${guild.name} has been unsuspended`,
        duration: 3000,
      });
    } finally {
      setActionLoading(false);
    }
  };

  // Handle delete action
  const handleDelete = async () => {
    const guild = selectedGuild();
    if (!guild) return;

    setActionLoading(true);
    try {
      const success = await deleteGuild(guild.id);
      if (success) {
        setShowDeleteDialog(false);
        setDeleteConfirmText("");
        showToast({
          type: "success",
          title: "Guild deleted",
          message: `${guild.name} has been permanently deleted`,
          duration: 5000,
        });
      }
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

  // Handle keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const guilds = filteredGuilds();
    if (guilds.length === 0) return;

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((prev) => {
          const next = prev < guilds.length - 1 ? prev + 1 : prev;
          selectGuild(guilds[next].id);
          return next;
        });
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex((prev) => {
          const next = prev > 0 ? prev - 1 : 0;
          selectGuild(guilds[next].id);
          return next;
        });
        break;
      case "Enter":
        if (focusedIndex() >= 0 && focusedIndex() < guilds.length) {
          selectGuild(guilds[focusedIndex()].id);
        }
        break;
      case "Escape":
        selectGuild(null);
        setFocusedIndex(-1);
        break;
    }
  };

  return (
    <div class="flex flex-1 h-full overflow-hidden">
      {/* Guild List */}
      <div class="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 class="text-lg font-bold text-text-primary">Guilds</h2>

          <div class="flex items-center gap-3">
            {/* Export Button */}
            <button
              onClick={handleExport}
              disabled={adminState.isExporting}
              class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/10 text-text-secondary hover:text-text-primary hover:bg-white/20 transition-colors disabled:opacity-50 disabled:cursor-not-allowed text-sm"
            >
              <Show
                when={!adminState.isExporting}
                fallback={<Loader2 class="w-4 h-4 animate-spin" />}
              >
                <Download class="w-4 h-4" />
              </Show>
              Export CSV
            </button>

            {/* Search Input */}
            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="text"
                placeholder="Search guilds..."
                value={searchQuery()}
                onInput={(e) => handleSearchInput(e.currentTarget.value)}
                class="pl-9 pr-4 py-2 w-64 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm"
              />
              <Show when={adminState.isGuildsLoading && searchQuery()}>
                <div class="absolute right-3 top-1/2 -translate-y-1/2">
                  <div class="w-4 h-4 border-2 border-accent-primary/30 border-t-accent-primary rounded-full animate-spin" />
                </div>
              </Show>
            </div>
          </div>
        </div>

        {/* Guild Table */}
        <div
          ref={listRef}
          class="flex-1 overflow-auto focus:outline-none"
          tabIndex={0}
          onKeyDown={handleKeyDown}
        >
          {/* Bulk Action Bar */}
          <Show when={getSelectedGuildCount() > 0}>
            <div class="flex items-center justify-between px-4 py-3 bg-accent-primary/20 border-b border-accent-primary/30">
              <div class="flex items-center gap-3">
                <span class="text-sm font-medium text-text-primary">
                  {getSelectedGuildCount()} guild
                  {getSelectedGuildCount() !== 1 ? "s" : ""} selected
                </span>
                <button
                  onClick={clearGuildSelection}
                  class="text-sm text-text-secondary hover:text-text-primary transition-colors"
                >
                  Clear selection
                </button>
              </div>
              <div class="flex items-center gap-2">
                <button
                  onClick={() => setShowBulkSuspendDialog(true)}
                  disabled={
                    !adminState.isElevated || adminState.isBulkActionLoading
                  }
                  class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-status-error text-white text-sm font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Ban class="w-4 h-4" />
                  Bulk Suspend
                </button>
              </div>
            </div>
          </Show>

          {/* Table Header */}
          <div class="grid grid-cols-[auto_1fr_1fr_1fr_1fr] gap-4 px-4 py-3 border-b border-white/10 bg-surface-layer1 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0 z-10">
            <div class="flex items-center">
              <button
                onClick={() =>
                  allSelected() ? clearGuildSelection() : selectAllGuilds()
                }
                class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                title={allSelected() ? "Deselect all" : "Select all"}
              >
                <Show
                  when={allSelected()}
                  fallback={<Square class="w-4 h-4" />}
                >
                  <CheckSquare class="w-4 h-4 text-accent-primary" />
                </Show>
              </button>
            </div>
            <div>Name</div>
            <div>Members</div>
            <div>Created</div>
            <div>Status</div>
          </div>

          {/* Loading State */}
          <Show when={adminState.isGuildsLoading}>
            <TableRowSkeleton columns={4} rows={8} showAvatar />
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
              {(guild, index) => (
                <div
                  onClick={() => {
                    selectGuild(guild.id);
                    setFocusedIndex(index());
                  }}
                  class="grid grid-cols-[auto_1fr_1fr_1fr_1fr] gap-4 px-4 py-3 border-b border-white/5 cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/20":
                      adminState.selectedGuildId === guild.id,
                    "hover:bg-white/5": adminState.selectedGuildId !== guild.id,
                    "ring-2 ring-accent-primary/50 ring-inset":
                      focusedIndex() === index() &&
                      adminState.selectedGuildId !== guild.id,
                  }}
                >
                  {/* Checkbox */}
                  <div class="flex items-center">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleGuildSelection(guild.id);
                      }}
                      class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                    >
                      <Show
                        when={isGuildSelected(guild.id)}
                        fallback={<Square class="w-4 h-4" />}
                      >
                        <CheckSquare class="w-4 h-4 text-accent-primary" />
                      </Show>
                    </button>
                  </div>

                  {/* Name */}
                  <div class="flex items-center gap-3 min-w-0">
                    <Avatar src={guild.icon_url} alt={guild.name} size="sm" />
                    <div class="text-sm font-medium text-text-primary truncate">
                      {guild.name}
                    </div>
                  </div>

                  {/* Members */}
                  <div
                    class="flex items-center gap-2 text-sm"
                    classList={{
                      "text-text-primary":
                        adminState.selectedGuildId === guild.id,
                      "text-text-secondary":
                        adminState.selectedGuildId !== guild.id,
                    }}
                  >
                    <Users class="w-4 h-4" />
                    {guild.member_count}
                  </div>

                  {/* Created */}
                  <div
                    class="flex items-center text-sm"
                    classList={{
                      "text-text-primary":
                        adminState.selectedGuildId === guild.id,
                      "text-text-secondary":
                        adminState.selectedGuildId !== guild.id,
                    }}
                  >
                    {formatDate(guild.created_at)}
                  </div>

                  {/* Status */}
                  <div class="flex items-center">
                    <Show
                      when={guild.suspended_at}
                      fallback={
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-accent-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-accent-danger">
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
                <Avatar src={guild().icon_url} alt={guild().name} size="lg" />
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

                {/* Owner Info - from guild details */}
                <div class="space-y-2">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Owner
                  </div>
                  <Show
                    when={
                      !adminState.isGuildDetailsLoading &&
                      adminState.selectedGuildDetails?.owner
                    }
                    fallback={
                      <Show
                        when={adminState.isGuildDetailsLoading}
                        fallback={
                          <div class="text-sm text-text-primary font-mono">
                            {truncateId(guild().owner_id)}
                          </div>
                        }
                      >
                        <Loader2 class="w-4 h-4 animate-spin text-text-secondary" />
                      </Show>
                    }
                  >
                    <div class="flex items-center gap-2 p-2 rounded-lg bg-white/5">
                      <Avatar
                        src={adminState.selectedGuildDetails!.owner.avatar_url}
                        alt={
                          adminState.selectedGuildDetails!.owner.display_name
                        }
                        size="sm"
                      />
                      <div class="flex-1 min-w-0">
                        <div class="flex items-center gap-1.5">
                          <span class="text-sm font-medium text-text-primary truncate">
                            {
                              adminState.selectedGuildDetails!.owner
                                .display_name
                            }
                          </span>
                          <Crown class="w-3 h-3 text-amber-400 flex-shrink-0" />
                        </div>
                        <div class="text-xs text-text-secondary">
                          @{adminState.selectedGuildDetails!.owner.username}
                        </div>
                      </div>
                    </div>
                  </Show>
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
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-accent-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-accent-danger">
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

                {/* Member Preview - from guild details */}
                <div class="space-y-2">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Members Preview
                  </div>
                  <Show
                    when={!adminState.isGuildDetailsLoading}
                    fallback={
                      <Loader2 class="w-4 h-4 animate-spin text-text-secondary" />
                    }
                  >
                    {/* Stacked Avatars */}
                    <div class="flex items-center">
                      <div class="flex -space-x-2">
                        <Show when={adminState.selectedGuildDetails?.owner}>
                          <div
                            class="relative ring-2 ring-[var(--color-surface-layer1)] rounded-full"
                            title={`${adminState.selectedGuildDetails!.owner.display_name} (@${adminState.selectedGuildDetails!.owner.username}) - Owner`}
                          >
                            <Avatar
                              src={
                                adminState.selectedGuildDetails!.owner
                                  .avatar_url
                              }
                              alt={
                                adminState.selectedGuildDetails!.owner
                                  .display_name
                              }
                              size="sm"
                            />
                          </div>
                        </Show>
                        <For
                          each={adminState.selectedGuildDetails?.top_members.slice(
                            0,
                            5,
                          )}
                        >
                          {(member) => (
                            <div
                              class="relative ring-2 ring-[var(--color-surface-layer1)] rounded-full"
                              title={`${member.display_name} (@${member.username})`}
                            >
                              <Avatar
                                src={member.avatar_url}
                                alt={member.display_name}
                                size="sm"
                              />
                            </div>
                          )}
                        </For>
                      </div>
                      <Show
                        when={adminState.selectedGuildDetails!.member_count > 6}
                      >
                        <span class="ml-2 text-xs text-text-secondary">
                          +{adminState.selectedGuildDetails!.member_count - 6}{" "}
                          more
                        </span>
                      </Show>
                    </div>

                    {/* Member List */}
                    <div class="space-y-2 max-h-40 overflow-y-auto">
                      <For each={adminState.selectedGuildDetails?.top_members}>
                        {(member) => (
                          <div class="flex items-center gap-2 p-2 rounded-lg bg-white/5">
                            <Avatar
                              src={member.avatar_url}
                              alt={member.display_name}
                              size="sm"
                            />
                            <div class="flex-1 min-w-0">
                              <div class="text-sm font-medium text-text-primary truncate">
                                {member.display_name}
                              </div>
                              <div class="text-xs text-text-secondary">
                                Joined {formatDate(member.joined_at)}
                              </div>
                            </div>
                          </div>
                        )}
                      </For>
                    </div>
                  </Show>
                </div>
              </div>

              {/* Actions Section */}
              <div class="space-y-3 pt-4 border-t border-white/10">
                <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  Actions
                </div>

                <Show when={!adminState.isElevated}>
                  <div class="p-3 rounded-lg bg-status-warning/10 border border-status-warning/30 text-text-primary text-xs">
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

                <button
                  onClick={() => setShowDeleteDialog(true)}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-status-error/50 text-status-error font-medium transition-colors hover:bg-status-error/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Trash2 class="w-4 h-4" />
                  Delete Guild
                </button>
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
              <h3 class="text-lg font-bold text-text-primary">Suspend Guild</h3>

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

      {/* Bulk Suspend Dialog */}
      <Show when={showBulkSuspendDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setShowBulkSuspendDialog(false)}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-text-primary">
                Bulk Suspend Guilds
              </h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to suspend{" "}
                <span class="font-medium text-text-primary">
                  {getSelectedGuildCount()} guild
                  {getSelectedGuildCount() !== 1 ? "s" : ""}
                </span>
                ? All members will be unable to access these guilds.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Reason for suspension (applies to all)
                </label>
                <textarea
                  value={bulkSuspendReason()}
                  onInput={(e) => setBulkSuspendReason(e.currentTarget.value)}
                  placeholder="Enter reason..."
                  rows={3}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm resize-none"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowBulkSuspendDialog(false);
                    setBulkSuspendReason("");
                  }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleBulkSuspend}
                  disabled={!bulkSuspendReason().trim() || actionLoading()}
                  class="flex-1 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {actionLoading()
                    ? "Suspending..."
                    : `Suspend ${getSelectedGuildCount()} Guilds`}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      {/* Delete Guild Dialog */}
      <Show when={showDeleteDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => {
              setShowDeleteDialog(false);
              setDeleteConfirmText("");
            }}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-status-error">Delete Guild</h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to permanently delete{" "}
                <span class="font-medium text-text-primary">
                  {selectedGuild()?.name}
                </span>
                ? This action is{" "}
                <span class="font-bold text-status-error">irreversible</span>{" "}
                and will remove all channels, messages, roles, and member data.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Type{" "}
                  <span class="font-mono text-text-primary">
                    {selectedGuild()?.name}
                  </span>{" "}
                  to confirm
                </label>
                <input
                  type="text"
                  value={deleteConfirmText()}
                  onInput={(e) => setDeleteConfirmText(e.currentTarget.value)}
                  placeholder={selectedGuild()?.name}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-status-error text-sm"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowDeleteDialog(false);
                    setDeleteConfirmText("");
                  }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleDelete}
                  disabled={
                    deleteConfirmText() !== selectedGuild()?.name ||
                    actionLoading()
                  }
                  class="flex-1 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {actionLoading() ? "Deleting..." : "Delete Permanently"}
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
