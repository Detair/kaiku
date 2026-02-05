/**
 * UsersPanel - User management panel for admin dashboard
 *
 * Provides user listing with search, pagination, and ban/unban functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import { Component, Show, For, onMount, createSignal, createMemo, onCleanup, createEffect } from "solid-js";
import { Search, Ban, CheckCircle, ChevronLeft, ChevronRight, X, Clock, Crown, Loader2, Download, Square, CheckSquare, Trash2 } from "lucide-solid";
import {
  adminState,
  loadUsers,
  selectUser,
  loadUserDetails,
  banUser,
  unbanUser,
  deleteUser,
  searchUsers,
  toggleUserSelection,
  selectAllUsers,
  clearUserSelection,
  isUserSelected,
  getSelectedUserCount,
  exportUsersCsv,
  bulkBanUsers,
} from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";
import TableRowSkeleton from "./TableRowSkeleton";
import { showToast } from "@/components/ui/Toast";

const PAGE_SIZE = 20;

const DEBOUNCE_MS = 300;

const UsersPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [banReason, setBanReason] = createSignal("");
  const [showBanDialog, setShowBanDialog] = createSignal(false);
  const [showBulkBanDialog, setShowBulkBanDialog] = createSignal(false);
  const [bulkBanReason, setBulkBanReason] = createSignal("");
  const [showDeleteDialog, setShowDeleteDialog] = createSignal(false);
  const [deleteConfirmText, setDeleteConfirmText] = createSignal("");
  const [actionLoading, setActionLoading] = createSignal(false);
  const [focusedIndex, setFocusedIndex] = createSignal(-1);

  let listRef: HTMLDivElement | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Check if all users on current page are selected
  const allSelected = createMemo(() => {
    const users = adminState.users;
    return users.length > 0 && users.every((u) => isUserSelected(u.id));
  });

  // Handle export
  const handleExport = async () => {
    await exportUsersCsv();
  };

  // Handle bulk ban
  const handleBulkBan = async () => {
    if (!bulkBanReason().trim()) return;

    setActionLoading(true);
    try {
      const result = await bulkBanUsers(bulkBanReason());
      if (result) {
        setShowBulkBanDialog(false);
        setBulkBanReason("");
      }
    } finally {
      setActionLoading(false);
    }
  };

  // Load users on mount
  onMount(() => {
    loadUsers(1);
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
      searchUsers(value);
    }, DEBOUNCE_MS);
  };

  // Calculate total pages
  const totalPages = createMemo(() =>
    Math.ceil(adminState.usersPagination.total / PAGE_SIZE) || 1
  );

  // Get currently selected user
  const selectedUser = createMemo(() =>
    adminState.users.find((u) => u.id === adminState.selectedUserId) ?? null
  );

  // Load user details when a user is selected
  createEffect(() => {
    const userId = adminState.selectedUserId;
    if (userId) {
      loadUserDetails(userId);
    }
  });

  // Users are now filtered server-side
  const filteredUsers = createMemo(() => adminState.users);

  // Handle page navigation
  const goToPage = (page: number) => {
    if (page >= 1 && page <= totalPages()) {
      loadUsers(page);
    }
  };

  // Handle ban action
  const handleBan = async () => {
    const user = selectedUser();
    if (!user || !banReason().trim()) return;

    setActionLoading(true);
    try {
      await banUser(user.id, banReason());
      setShowBanDialog(false);
      setBanReason("");

      // Show undo toast
      const username = user.username;
      const userId = user.id;
      showToast({
        id: `ban-undo-${userId}`,
        type: "success",
        title: "User banned",
        message: `@${username} has been banned`,
        duration: 5000,
        action: {
          label: "Undo",
          onClick: async () => {
            await unbanUser(userId);
            showToast({
              type: "info",
              title: "Ban reversed",
              message: `@${username} has been unbanned`,
              duration: 3000,
            });
          },
        },
      });
    } finally {
      setActionLoading(false);
    }
  };

  // Handle unban action
  const handleUnban = async () => {
    const user = selectedUser();
    if (!user) return;

    setActionLoading(true);
    try {
      await unbanUser(user.id);
      showToast({
        type: "success",
        title: "User unbanned",
        message: `@${user.username} has been unbanned`,
        duration: 3000,
      });
    } finally {
      setActionLoading(false);
    }
  };

  // Handle delete action
  const handleDelete = async () => {
    const user = selectedUser();
    if (!user) return;

    setActionLoading(true);
    try {
      const success = await deleteUser(user.id);
      if (success) {
        setShowDeleteDialog(false);
        setDeleteConfirmText("");
        showToast({
          type: "success",
          title: "User deleted",
          message: `@${user.username} has been permanently deleted`,
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

  // Handle keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const users = filteredUsers();
    if (users.length === 0) return;

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((prev) => {
          const next = prev < users.length - 1 ? prev + 1 : prev;
          selectUser(users[next].id);
          return next;
        });
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex((prev) => {
          const next = prev > 0 ? prev - 1 : 0;
          selectUser(users[next].id);
          return next;
        });
        break;
      case "Enter":
        if (focusedIndex() >= 0 && focusedIndex() < users.length) {
          selectUser(users[focusedIndex()].id);
        }
        break;
      case "Escape":
        selectUser(null);
        setFocusedIndex(-1);
        break;
    }
  };

  return (
    <div class="flex flex-1 h-full overflow-hidden">
      {/* User List */}
      <div class="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 class="text-lg font-bold text-text-primary">Users</h2>

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
                placeholder="Search users..."
                value={searchQuery()}
                onInput={(e) => handleSearchInput(e.currentTarget.value)}
                class="pl-9 pr-4 py-2 w-64 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm"
              />
              <Show when={adminState.isUsersLoading && searchQuery()}>
                <div class="absolute right-3 top-1/2 -translate-y-1/2">
                  <div class="w-4 h-4 border-2 border-accent-primary/30 border-t-accent-primary rounded-full animate-spin" />
                </div>
              </Show>
            </div>
          </div>
        </div>

        {/* User Table */}
        <div
          ref={listRef}
          class="flex-1 overflow-auto focus:outline-none"
          tabIndex={0}
          onKeyDown={handleKeyDown}
        >
        {/* Bulk Action Bar */}
        <Show when={getSelectedUserCount() > 0}>
          <div class="flex items-center justify-between px-4 py-3 bg-accent-primary/20 border-b border-accent-primary/30">
            <div class="flex items-center gap-3">
              <span class="text-sm font-medium text-text-primary">
                {getSelectedUserCount()} user{getSelectedUserCount() !== 1 ? "s" : ""} selected
              </span>
              <button
                onClick={clearUserSelection}
                class="text-sm text-text-secondary hover:text-text-primary transition-colors"
              >
                Clear selection
              </button>
            </div>
            <div class="flex items-center gap-2">
              <button
                onClick={() => setShowBulkBanDialog(true)}
                disabled={!adminState.isElevated || adminState.isBulkActionLoading}
                class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-status-error text-white text-sm font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Ban class="w-4 h-4" />
                Bulk Ban
              </button>
            </div>
          </div>
        </Show>

          {/* Table Header */}
          <div class="grid grid-cols-[auto_1fr_1fr_1fr_1fr] gap-4 px-4 py-3 border-b border-white/10 bg-white/5 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0">
            <div class="flex items-center">
              <button
                onClick={() => allSelected() ? clearUserSelection() : selectAllUsers()}
                class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                title={allSelected() ? "Deselect all" : "Select all"}
              >
                <Show when={allSelected()} fallback={<Square class="w-4 h-4" />}>
                  <CheckSquare class="w-4 h-4 text-accent-primary" />
                </Show>
              </button>
            </div>
            <div>Username</div>
            <div>Email</div>
            <div>Joined</div>
            <div>Status</div>
          </div>

          {/* Loading State */}
          <Show when={adminState.isUsersLoading}>
            <TableRowSkeleton columns={4} rows={8} showAvatar />
          </Show>

          {/* User Rows */}
          <Show when={!adminState.isUsersLoading}>
            <For
              each={filteredUsers()}
              fallback={
                <div class="flex items-center justify-center py-12">
                  <div class="text-text-secondary">No users found</div>
                </div>
              }
            >
              {(user, index) => (
                <div
                  onClick={() => {
                    selectUser(user.id);
                    setFocusedIndex(index());
                  }}
                  class="grid grid-cols-[auto_1fr_1fr_1fr_1fr] gap-4 px-4 py-3 border-b border-white/5 cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/20": adminState.selectedUserId === user.id,
                    "hover:bg-white/5": adminState.selectedUserId !== user.id,
                    "ring-2 ring-accent-primary/50 ring-inset": focusedIndex() === index() && adminState.selectedUserId !== user.id,
                  }}
                >
                  {/* Checkbox */}
                  <div class="flex items-center">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleUserSelection(user.id);
                      }}
                      class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                    >
                      <Show when={isUserSelected(user.id)} fallback={<Square class="w-4 h-4" />}>
                        <CheckSquare class="w-4 h-4 text-accent-primary" />
                      </Show>
                    </button>
                  </div>

                  {/* Username */}
                  <div class="flex items-center gap-3 min-w-0 relative z-10">
                    <Avatar
                      src={user.avatar_url}
                      alt={user.display_name || user.username}
                      size="sm"
                    />
                    <div class="min-w-0">
                      <div class="text-sm font-medium text-text-primary truncate">
                        {user.display_name}
                      </div>
                      <div
                        class="text-xs truncate"
                        classList={{
                          "text-text-primary": adminState.selectedUserId === user.id,
                          "text-text-secondary": adminState.selectedUserId !== user.id,
                        }}
                      >
                        @{user.username}
                      </div>
                    </div>
                  </div>

                  {/* Email */}
                  <div
                    class="flex items-center text-sm truncate"
                    classList={{
                      "text-text-primary": adminState.selectedUserId === user.id,
                      "text-text-secondary": adminState.selectedUserId !== user.id,
                    }}
                  >
                    {user.email || "-"}
                  </div>

                  {/* Joined */}
                  <div
                    class="flex items-center text-sm"
                    classList={{
                      "text-text-primary": adminState.selectedUserId === user.id,
                      "text-text-secondary": adminState.selectedUserId !== user.id,
                    }}
                  >
                    {formatDate(user.created_at)}
                  </div>

                  {/* Status */}
                  <div class="flex items-center">
                    <Show
                      when={user.is_banned}
                      fallback={
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-status-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-status-error">
                        <Ban class="w-3 h-3" />
                        Banned
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
            {adminState.usersPagination.total} total users
          </div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => goToPage(adminState.usersPagination.page - 1)}
              disabled={adminState.usersPagination.page <= 1}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-sm text-text-primary">
              Page {adminState.usersPagination.page} of {totalPages()}
            </span>
            <button
              onClick={() => goToPage(adminState.usersPagination.page + 1)}
              disabled={adminState.usersPagination.page >= totalPages()}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronRight class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>

      {/* Detail Panel */}
      <Show when={selectedUser()}>
        {(user) => (
          <div class="w-80 flex-shrink-0 border-l border-white/10 flex flex-col">
            {/* Detail Header */}
            <div class="flex items-center justify-between p-4 border-b border-white/10">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                User Details
              </h3>
              <button
                onClick={() => selectUser(null)}
                class="p-1 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
              >
                <X class="w-4 h-4" />
              </button>
            </div>

            {/* Detail Content */}
            <div class="flex-1 p-4 space-y-6 overflow-auto">
              {/* Profile Section */}
              <div class="flex flex-col items-center text-center space-y-3">
                <Avatar
                  src={user().avatar_url}
                  alt={user().display_name || user().username}
                  size="lg"
                />
                <div>
                  <div class="text-lg font-bold text-text-primary">
                    {user().display_name}
                  </div>
                  <div class="text-sm text-text-secondary">@{user().username}</div>
                </div>
              </div>

              {/* Info Section */}
              <div class="space-y-4">
                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Email
                  </div>
                  <div class="text-sm text-text-primary">
                    {user().email || "Not provided"}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Joined
                  </div>
                  <div class="text-sm text-text-primary">
                    {formatDate(user().created_at)}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Status
                  </div>
                  <div>
                    <Show
                      when={user().is_banned}
                      fallback={
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-status-success">
                          <CheckCircle class="w-3 h-3" />
                          Active
                        </span>
                      }
                    >
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-status-error">
                        <Ban class="w-3 h-3" />
                        Banned
                      </span>
                    </Show>
                  </div>
                </div>

                {/* Last Login - from user details */}
                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Last Login
                  </div>
                  <div class="flex items-center gap-2 text-sm text-text-primary">
                    <Clock class="w-4 h-4 text-text-secondary" />
                    <Show
                      when={!adminState.isUserDetailsLoading}
                      fallback={<Loader2 class="w-4 h-4 animate-spin text-text-secondary" />}
                    >
                      {adminState.selectedUserDetails?.last_login
                        ? formatDate(adminState.selectedUserDetails.last_login)
                        : "Never"}
                    </Show>
                  </div>
                </div>

                {/* Guild Memberships - from user details */}
                <div class="space-y-2">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    Guilds ({adminState.selectedUserDetails?.guild_count ?? "-"})
                  </div>
                  <Show
                    when={!adminState.isUserDetailsLoading}
                    fallback={<Loader2 class="w-4 h-4 animate-spin text-text-secondary" />}
                  >
                    <Show
                      when={adminState.selectedUserDetails?.guilds.length}
                      fallback={
                        <div class="text-sm text-text-secondary">No guild memberships</div>
                      }
                    >
                      <div class="space-y-2 max-h-32 overflow-y-auto">
                        <For each={adminState.selectedUserDetails?.guilds}>
                          {(guild) => (
                            <div class="flex items-center gap-2 p-2 rounded-lg bg-white/5">
                              <Avatar
                                src={guild.guild_icon_url}
                                alt={guild.guild_name}
                                size="sm"
                              />
                              <div class="flex-1 min-w-0">
                                <div class="flex items-center gap-1.5">
                                  <span class="text-sm font-medium text-text-primary truncate">
                                    {guild.guild_name}
                                  </span>
                                  <Show when={guild.is_owner}>
                                    <Crown class="w-3 h-3 text-amber-400 flex-shrink-0" />
                                  </Show>
                                </div>
                                <div class="text-xs text-text-secondary">
                                  Joined {formatDate(guild.joined_at)}
                                </div>
                              </div>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  </Show>
                </div>
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
                  when={user().is_banned}
                  fallback={
                    <button
                      onClick={() => setShowBanDialog(true)}
                      disabled={!adminState.isElevated || actionLoading()}
                      class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      <Ban class="w-4 h-4" />
                      Ban User
                    </button>
                  }
                >
                  <button
                    onClick={handleUnban}
                    disabled={!adminState.isElevated || actionLoading()}
                    class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-status-success text-white font-medium transition-colors hover:bg-status-success/90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <CheckCircle class="w-4 h-4" />
                    {actionLoading() ? "Processing..." : "Unban User"}
                  </button>
                </Show>

                <button
                  onClick={() => setShowDeleteDialog(true)}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-status-error/50 text-status-error font-medium transition-colors hover:bg-status-error/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Trash2 class="w-4 h-4" />
                  Delete User
                </button>
              </div>
            </div>
          </div>
        )}
      </Show>

      {/* Ban Dialog */}
      <Show when={showBanDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setShowBanDialog(false)}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-text-primary">
                Ban User
              </h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to ban{" "}
                <span class="font-medium text-text-primary">
                  @{selectedUser()?.username}
                </span>
                ? They will be unable to access the platform.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Reason for ban
                </label>
                <textarea
                  value={banReason()}
                  onInput={(e) => setBanReason(e.currentTarget.value)}
                  placeholder="Enter reason..."
                  rows={3}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm resize-none"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowBanDialog(false);
                    setBanReason("");
                  }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleBan}
                  disabled={!banReason().trim() || actionLoading()}
                  class="flex-1 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {actionLoading() ? "Banning..." : "Confirm Ban"}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      {/* Bulk Ban Dialog */}
      <Show when={showBulkBanDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setShowBulkBanDialog(false)}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-text-primary">
                Bulk Ban Users
              </h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to ban{" "}
                <span class="font-medium text-text-primary">
                  {getSelectedUserCount()} user{getSelectedUserCount() !== 1 ? "s" : ""}
                </span>
                ? They will be unable to access the platform.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Reason for ban (applies to all)
                </label>
                <textarea
                  value={bulkBanReason()}
                  onInput={(e) => setBulkBanReason(e.currentTarget.value)}
                  placeholder="Enter reason..."
                  rows={3}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm resize-none"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowBulkBanDialog(false);
                    setBulkBanReason("");
                  }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleBulkBan}
                  disabled={!bulkBanReason().trim() || actionLoading()}
                  class="flex-1 px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {actionLoading() ? "Banning..." : `Ban ${getSelectedUserCount()} Users`}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      {/* Delete User Dialog */}
      <Show when={showDeleteDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => { setShowDeleteDialog(false); setDeleteConfirmText(""); }}
          />

          {/* Dialog */}
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="p-5 space-y-4">
              <h3 class="text-lg font-bold text-status-error">
                Delete User
              </h3>

              <p class="text-sm text-text-secondary">
                Are you sure you want to permanently delete{" "}
                <span class="font-medium text-text-primary">
                  @{selectedUser()?.username}
                </span>
                ? This action is <span class="font-bold text-status-error">irreversible</span> and will remove all their data including messages, guild memberships, and sessions.
              </p>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">
                  Type <span class="font-mono text-text-primary">{selectedUser()?.username}</span> to confirm
                </label>
                <input
                  type="text"
                  value={deleteConfirmText()}
                  onInput={(e) => setDeleteConfirmText(e.currentTarget.value)}
                  placeholder={selectedUser()?.username}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-status-error text-sm"
                />
              </div>

              <div class="flex gap-3 pt-2">
                <button
                  onClick={() => { setShowDeleteDialog(false); setDeleteConfirmText(""); }}
                  class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleDelete}
                  disabled={deleteConfirmText() !== selectedUser()?.username || actionLoading()}
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

export default UsersPanel;
