/**
 * UsersPanel - User management panel for admin dashboard
 *
 * Provides user listing with search, pagination, and ban/unban functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import { Component, Show, For, onMount, createSignal, createMemo, onCleanup } from "solid-js";
import { Search, Ban, CheckCircle, ChevronLeft, ChevronRight, X } from "lucide-solid";
import {
  adminState,
  loadUsers,
  selectUser,
  banUser,
  unbanUser,
  searchUsers,
} from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";
import TableRowSkeleton from "./TableRowSkeleton";

const PAGE_SIZE = 20;

const DEBOUNCE_MS = 300;

const UsersPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [banReason, setBanReason] = createSignal("");
  const [showBanDialog, setShowBanDialog] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal(false);
  const [focusedIndex, setFocusedIndex] = createSignal(-1);

  let listRef: HTMLDivElement | undefined;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

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

        {/* User Table */}
        <div
          ref={listRef}
          class="flex-1 overflow-auto focus:outline-none"
          tabIndex={0}
          onKeyDown={handleKeyDown}
        >
          {/* Table Header */}
          <div class="grid grid-cols-4 gap-4 px-4 py-3 border-b border-white/10 bg-white/5 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0">
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
                  class="grid grid-cols-4 gap-4 px-4 py-3 border-b border-white/5 cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/20": adminState.selectedUserId === user.id,
                    "hover:bg-white/5": adminState.selectedUserId !== user.id,
                    "ring-2 ring-accent-primary/50 ring-inset": focusedIndex() === index() && adminState.selectedUserId !== user.id,
                  }}
                >
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
    </div>
  );
};

export default UsersPanel;
