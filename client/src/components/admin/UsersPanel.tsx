/**
 * UsersPanel - User management panel for admin dashboard
 *
 * Provides user listing with search, pagination, and ban/unban functionality.
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
  X,
  Clock,
  Crown,
  Loader2,
  Download,
  Square,
  CheckSquare,
  Trash2,
} from "lucide-solid";
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
  ADMIN_MODE_MINUTES,
} from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";
import TableRowSkeleton from "./TableRowSkeleton";
import { showToast } from "@/components/ui/Toast";
import ConfirmActionDialog from "./ConfirmActionDialog";
import { actionErrorText } from "./actionErrors";
import { formatDay } from "./formatDate";
import CreateAccountModal from "./CreateAccountModal";
import UserSecurityPanel from "./UserSecurityPanel";
import AdminRequestsPanel from "./AdminRequestsPanel";
import { currentUser } from "@/stores/auth";
import { formatList } from "@/i18n/format";
import { plural, t } from "@/i18n";
import type { UserSummary } from "@/lib/types";

const PAGE_SIZE = 20;

const DEBOUNCE_MS = 300;

const UsersPanel: Component = () => {
  // The field starts from the stored search, so it never hides an active filter (ADMIN-18).
  const [searchQuery, setSearchQuery] = createSignal(adminState.usersSearch);
  const [banReason, setBanReason] = createSignal("");
  const [showBanDialog, setShowBanDialog] = createSignal(false);
  const [showBulkBanDialog, setShowBulkBanDialog] = createSignal(false);
  const [showUnbanDialog, setShowUnbanDialog] = createSignal(false);
  const [bulkBanReason, setBulkBanReason] = createSignal("");
  const [showDeleteDialog, setShowDeleteDialog] = createSignal(false);
  const [deleteConfirmText, setDeleteConfirmText] = createSignal("");
  const [bulkFailures, setBulkFailures] = createSignal<string[]>([]);
  // Names of the selected people, captured when the bulk dialog opens.
  const [bulkTargets, setBulkTargets] = createSignal<UserSummary[]>([]);
  const [focusedIndex, setFocusedIndex] = createSignal(-1);
  const [showCreateAccount, setShowCreateAccount] = createSignal(false);

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

  const nameOf = (u: Pick<UserSummary, "display_name" | "username">) =>
    u.display_name || u.username;

  const openBulkBan = () => {
    setBulkTargets(adminState.users.filter((u) => isUserSelected(u.id)));
    setBulkFailures([]);
    setShowBulkBanDialog(true);
  };

  // Bulk ban: names everyone, reports who could not be banned (ADMIN-7).
  const handleBulkBan = async () => {
    const targets = bulkTargets();
    const result = await bulkBanUsers(targets.map((u) => u.id), bulkBanReason());
    const failedIds = new Set(result.failed.map((f) => f.id));
    const failedNames = targets.filter((u) => failedIds.has(u.id)).map(nameOf);
    setBulkFailures(failedNames);
    setBulkBanReason("");
    showToast({
      type: failedNames.length > 0 ? "warning" : "success",
      title: plural(result.banned_count, { one: "admin.users.bulkBanned.one", other: "admin.users.bulkBanned.other" }),
      message:
        failedNames.length > 0
          ? t("admin.users.bulkFailed", { names: formatList(failedNames) })
          : undefined,
      duration: failedNames.length > 0 ? 10000 : 4000,
    });
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
  const totalPages = createMemo(
    () => Math.ceil(adminState.usersPagination.total / PAGE_SIZE) || 1,
  );

  // Get currently selected user
  const selectedUser = createMemo(
    () =>
      adminState.users.find((u) => u.id === adminState.selectedUserId) ?? null,
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

  // Ban: the dialog closes and the toast appears only after the server
  // banned the person; otherwise the dialog says why (ADMIN-1).
  const handleBan = async (user: UserSummary) => {
    await banUser(user.id, banReason());
    setBanReason("");
    const userId = user.id;
    const name = nameOf(user);
    showToast({
      id: `ban-undo-${userId}`,
      type: "success",
      title: t("admin.users.banned", { name }),
      duration: 5000,
      action: {
        label: t("admin.users.undo"),
        onClick: async () => {
          try {
            await unbanUser(userId);
            showToast({ type: "info", title: t("admin.users.unbanned", { name }), duration: 3000 });
          } catch (err) {
            showToast({ type: "error", title: t("admin.users.unbanFailed", { name }), message: actionErrorText(err, "admin.action.failed"), duration: 8000 });
          }
        },
      },
    });
  };

  const handleUnban = async (user: UserSummary) => {
    await unbanUser(user.id);
    showToast({ type: "success", title: t("admin.users.unbanned", { name: nameOf(user) }), duration: 3000 });
  };

  const handleDelete = async (user: UserSummary) => {
    await deleteUser(user.id);
    setDeleteConfirmText("");
    showToast({ type: "success", title: t("admin.users.deleted", { name: nameOf(user) }), duration: 5000 });
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

  const statusBadge = (banned: boolean) => (
    <Show
      when={banned}
      fallback={
        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-success/20 text-text-primary">
          <CheckCircle class="w-3 h-3" />
          {t("admin.users.status.active")}
        </span>
      }
    >
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-status-error/20 text-text-primary">
        <Ban class="w-3 h-3" />
        {t("admin.users.status.banned")}
      </span>
    </Show>
  );

  return (
    <div class="flex flex-1 h-full overflow-hidden">
      {/* User List */}
      <div class="flex-1 flex flex-col min-w-0">
        {/* Open requests to change sign-in (C27, elevated) */}
        <Show when={adminState.isElevated}>
          <AdminRequestsPanel />
        </Show>
        {/* Header */}
        <div class="flex flex-wrap items-center justify-between gap-3 p-4 border-b border-white/10">
          <h2 class="text-lg font-bold text-text-primary">{t("admin.users.title")}</h2>

          <div class="flex flex-wrap items-center gap-3">
            <button
              onClick={() => setShowCreateAccount(true)}
              class="flex items-center gap-2 px-3 py-2 rounded-lg bg-accent-primary hover:bg-accent-primary/80 text-on-accent transition-colors text-sm font-medium"
            >
              {t("admin.createAccount.title")}
            </button>

            <button
              onClick={handleExport}
              disabled={adminState.isExporting}
              class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/10 text-text-secondary hover:text-text-primary hover:bg-white/20 transition-colors text-sm state-disabled"
            >
              <Show
                when={!adminState.isExporting}
                fallback={<Loader2 class="w-4 h-4 animate-spin" />}
              >
                <Download class="w-4 h-4" />
              </Show>
              {t("admin.users.export")}
            </button>

            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="search"
                aria-label={t("admin.users.search")}
                placeholder={t("admin.users.search")}
                value={searchQuery()}
                onInput={(e) => handleSearchInput(e.currentTarget.value)}
                class="pl-9 pr-4 py-2 w-64 max-w-full rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary text-sm"
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
          aria-label={t("admin.users.listLabel")}
          onKeyDown={handleKeyDown}
        >
          {/* Bulk Action Bar */}
          <Show when={getSelectedUserCount() > 0}>
            <div class="flex flex-wrap items-center justify-between gap-2 px-4 py-3 bg-accent-primary/20 border-b border-accent-primary/30">
              <div class="flex items-center gap-3">
                <span class="text-sm font-medium text-text-primary">
                  {plural(getSelectedUserCount(), { one: "admin.users.selected.one", other: "admin.users.selected.other" })}
                </span>
                <button
                  onClick={clearUserSelection}
                  class="text-sm text-text-secondary hover:text-text-primary transition-colors"
                >
                  {t("admin.users.clearSelection")}
                </button>
              </div>
              <div class="flex items-center gap-2">
                <button
                  onClick={openBulkBan}
                  disabled={
                    !adminState.isElevated || adminState.isBulkActionLoading
                  }
                  class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-danger-fill text-on-danger text-sm font-medium transition-colors hover:bg-accent-danger-fill/90 state-disabled"
                >
                  <Ban class="w-4 h-4" />
                  {t("admin.users.bulkBan")}
                </button>
              </div>
            </div>
          </Show>

          {/* Table Header */}
          <div class="grid grid-cols-[auto_1fr_1fr_1fr_1fr] gap-4 px-4 py-3 border-b border-white/10 bg-white/5 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0">
            <div class="flex items-center">
              <button
                onClick={() =>
                  allSelected() ? clearUserSelection() : selectAllUsers()
                }
                class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                title={allSelected() ? t("admin.users.deselectPage") : t("admin.users.selectPage")}
                aria-label={allSelected() ? t("admin.users.deselectPage") : t("admin.users.selectPage")}
                aria-pressed={allSelected()}
              >
                <Show
                  when={allSelected()}
                  fallback={<Square class="w-4 h-4" />}
                >
                  <CheckSquare class="w-4 h-4 text-accent-primary" />
                </Show>
              </button>
            </div>
            <div>{t("admin.users.col.name")}</div>
            <div>{t("admin.users.col.email")}</div>
            <div>{t("admin.users.col.joined")}</div>
            <div>{t("admin.users.col.status")}</div>
          </div>

          {/* Loading State */}
          <Show when={adminState.isUsersLoading}>
            <TableRowSkeleton columns={4} rows={8} showAvatar />
          </Show>

          {/* A failed load is not an empty list (ADMIN-18). */}
          <Show when={!adminState.isUsersLoading && adminState.usersLoadFailed}>
            <div role="alert" class="flex flex-col items-center gap-3 py-12 text-text-primary">
              <div>{t("admin.users.loadFailed")}</div>
              <button
                onClick={() => loadUsers(adminState.usersPagination.page)}
                class="px-3 py-1.5 rounded-lg bg-white/10 text-sm hover:bg-white/20"
              >
                {t("common.tryAgain")}
              </button>
            </div>
          </Show>

          {/* User Rows */}
          <Show when={!adminState.isUsersLoading && !adminState.usersLoadFailed}>
            <For
              each={filteredUsers()}
              fallback={
                <div class="flex items-center justify-center py-12">
                  <div class="text-text-secondary">
                    {adminState.usersSearch ? t("admin.users.noMatch", { query: adminState.usersSearch }) : t("admin.users.empty")}
                  </div>
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
                    "bg-accent-primary/20":
                      adminState.selectedUserId === user.id,
                    "hover:bg-white/5": adminState.selectedUserId !== user.id,
                    "ring-2 ring-accent-primary/50 ring-inset":
                      focusedIndex() === index() &&
                      adminState.selectedUserId !== user.id,
                  }}
                >
                  {/* Checkbox */}
                  <div class="flex items-center">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleUserSelection(user.id);
                      }}
                      aria-label={t("admin.users.selectNamed", { name: nameOf(user) })}
                      aria-pressed={isUserSelected(user.id)}
                      class="p-1 text-text-secondary hover:text-text-primary transition-colors"
                    >
                      <Show
                        when={isUserSelected(user.id)}
                        fallback={<Square class="w-4 h-4" />}
                      >
                        <CheckSquare class="w-4 h-4 text-accent-primary" />
                      </Show>
                    </button>
                  </div>

                  {/* Username */}
                  <div class="flex items-center gap-3 min-w-0 relative z-10">
                    <Avatar
                      src={user.avatar_url}
                      alt={nameOf(user)}
                      size="sm"
                    />
                    <div class="min-w-0">
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          selectUser(user.id);
                          setFocusedIndex(index());
                        }}
                        class="block text-left text-sm font-medium text-text-primary truncate max-w-full hover:underline"
                      >
                        {user.display_name}
                      </button>
                      <div
                        class="text-xs truncate"
                        classList={{
                          "text-text-primary":
                            adminState.selectedUserId === user.id,
                          "text-text-secondary":
                            adminState.selectedUserId !== user.id,
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
                      "text-text-primary":
                        adminState.selectedUserId === user.id,
                      "text-text-secondary":
                        adminState.selectedUserId !== user.id,
                    }}
                  >
                    {user.email || "-"}
                  </div>

                  {/* Joined */}
                  <div
                    class="flex items-center text-sm"
                    classList={{
                      "text-text-primary":
                        adminState.selectedUserId === user.id,
                      "text-text-secondary":
                        adminState.selectedUserId !== user.id,
                    }}
                  >
                    {formatDay(user.created_at)}
                  </div>

                  {/* Status */}
                  <div class="flex items-center">{statusBadge(user.is_banned)}</div>
                </div>
              )}
            </For>
          </Show>
        </div>

        {/* Pagination */}
        <div class="flex items-center justify-between px-4 py-3 border-t border-white/10">
          <div class="text-sm text-text-secondary">
            {plural(adminState.usersPagination.total, { one: "admin.users.total.one", other: "admin.users.total.other" })}
          </div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => goToPage(adminState.usersPagination.page - 1)}
              disabled={adminState.usersPagination.page <= 1}
              aria-label={t("admin.users.prevPage")}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors state-disabled-quiet"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-sm text-text-primary">
              {t("admin.users.page", { page: adminState.usersPagination.page, total: totalPages() })}
            </span>
            <button
              onClick={() => goToPage(adminState.usersPagination.page + 1)}
              disabled={adminState.usersPagination.page >= totalPages()}
              aria-label={t("admin.users.nextPage")}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors state-disabled-quiet"
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
            <div class="flex items-center justify-between p-4 border-b border-white/10">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                {t("admin.users.details")}
              </h3>
              <button
                onClick={() => selectUser(null)}
                aria-label={t("common.close")}
                class="p-1 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
              >
                <X class="w-4 h-4" />
              </button>
            </div>

            <div class="flex-1 p-4 space-y-6 overflow-auto">
              <div class="flex flex-col items-center text-center space-y-3">
                <Avatar
                  src={user().avatar_url}
                  alt={nameOf(user())}
                  size="lg"
                />
                <div>
                  <div class="text-lg font-bold text-text-primary">
                    {user().display_name}
                  </div>
                  <div class="text-sm text-text-secondary">
                    @{user().username}
                  </div>
                </div>
              </div>

              <div class="space-y-4">
                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    {t("admin.users.col.email")}
                  </div>
                  <div class="text-sm text-text-primary">
                    {user().email || t("admin.users.noEmail")}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    {t("admin.users.col.joined")}
                  </div>
                  <div class="text-sm text-text-primary">
                    {formatDay(user().created_at)}
                  </div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    {t("admin.users.col.status")}
                  </div>
                  <div>{statusBadge(user().is_banned)}</div>
                </div>

                <div class="space-y-1">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    {t("admin.users.lastLogin")}
                  </div>
                  <div class="flex items-center gap-2 text-sm text-text-primary">
                    <Clock class="w-4 h-4 text-text-secondary" />
                    <Show
                      when={!adminState.isUserDetailsLoading}
                      fallback={
                        <Loader2 class="w-4 h-4 animate-spin text-text-secondary" />
                      }
                    >
                      {adminState.selectedUserDetails?.last_login
                        ? formatDay(adminState.selectedUserDetails.last_login)
                        : t("admin.users.never")}
                    </Show>
                  </div>
                </div>

                <div class="space-y-2">
                  <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                    {t("admin.users.servers", { count: adminState.selectedUserDetails?.guild_count ?? "-" })}
                  </div>
                  <Show
                    when={!adminState.isUserDetailsLoading}
                    fallback={
                      <Loader2 class="w-4 h-4 animate-spin text-text-secondary" />
                    }
                  >
                    <Show
                      when={adminState.selectedUserDetails?.guilds.length}
                      fallback={
                        <div class="text-sm text-text-secondary">
                          {t("admin.users.noServers")}
                        </div>
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
                                    <Crown
                                      class="w-3 h-3 text-status-warning flex-shrink-0"
                                      aria-label={t("admin.users.owner")}
                                    />
                                  </Show>
                                </div>
                                <div class="text-xs text-text-secondary">
                                  {t("admin.users.joinedOn", { date: formatDay(guild.joined_at) })}
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

              {/* Sign-in security (C27 admin user view) */}
              {/* Keyed: another user gets a fresh panel (its toggles start from that user). */}
              <Show when={adminState.selectedUserDetails} keyed>
                {(details) => <UserSecurityPanel user={details} meId={currentUser()?.id ?? ""} />}
              </Show>

              {/* Actions Section */}
              <div class="space-y-3 pt-4 border-t border-white/10">
                <div class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  {t("admin.users.actions")}
                </div>

                <Show when={!adminState.isElevated}>
                  <div class="p-3 rounded-lg bg-status-warning/10 border border-status-warning/30 text-text-primary text-xs">
                    {t("admin.mode.needed", { minutes: ADMIN_MODE_MINUTES })}
                  </div>
                </Show>

                <Show
                  when={user().is_banned}
                  fallback={
                    <button
                      onClick={() => setShowBanDialog(true)}
                      disabled={!adminState.isElevated}
                      class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-accent-danger-fill text-on-danger font-medium transition-colors hover:bg-accent-danger-fill/90 state-disabled"
                    >
                      <Ban class="w-4 h-4" />
                      {t("admin.users.ban")}
                    </button>
                  }
                >
                  <button
                    onClick={() => setShowUnbanDialog(true)}
                    disabled={!adminState.isElevated}
                    class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-status-success text-on-success font-medium transition-colors hover:bg-status-success/90 state-disabled"
                  >
                    <CheckCircle class="w-4 h-4" />
                    {t("admin.users.unban")}
                  </button>
                </Show>

                <button
                  onClick={() => setShowDeleteDialog(true)}
                  disabled={!adminState.isElevated}
                  class="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-status-error/50 text-text-primary font-medium transition-colors hover:bg-status-error/10 state-disabled-quiet"
                >
                  <Trash2 class="w-4 h-4" />
                  {t("admin.users.delete")}
                </button>
              </div>
            </div>
          </div>
        )}
      </Show>

      {/* Ban: stays open with the reason when it fails (ADMIN-1). */}
      <Show when={showBanDialog() && selectedUser()}>
        {(user) => (
          <ConfirmActionDialog
            title={t("admin.users.banTitle", { name: nameOf(user()) })}
            message={t("admin.users.banBody", { name: nameOf(user()) })}
            confirmLabel={t("admin.users.ban")}
            danger
            confirmDisabled={!banReason().trim()}
            failureFallback="admin.action.failed"
            onConfirm={() => handleBan(user())}
            onDone={() => setShowBanDialog(false)}
            onCancel={() => {
              setShowBanDialog(false);
              setBanReason("");
            }}
            testId="ban-dialog"
          >
            <label class="block space-y-2">
              <span class="text-sm font-medium text-text-secondary">{t("admin.users.reason")}</span>
              <textarea
                value={banReason()}
                onInput={(e) => setBanReason(e.currentTarget.value)}
                placeholder={t("admin.users.reasonPlaceholder")}
                rows={3}
                class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary text-sm resize-none"
              />
            </label>
          </ConfirmActionDialog>
        )}
      </Show>

      {/* Unban asks too, so every admin action reads the same (ADMIN-13). */}
      <Show when={showUnbanDialog() && selectedUser()}>
        {(user) => (
          <ConfirmActionDialog
            title={t("admin.users.unbanTitle", { name: nameOf(user()) })}
            message={t("admin.users.unbanBody", { name: nameOf(user()) })}
            confirmLabel={t("admin.users.unban")}
            failureFallback="admin.action.failed"
            onConfirm={() => handleUnban(user())}
            onDone={() => setShowUnbanDialog(false)}
            onCancel={() => setShowUnbanDialog(false)}
            testId="unban-dialog"
          />
        )}
      </Show>

      {/* Bulk ban: names everyone it hits (ADMIN-7). */}
      <Show when={showBulkBanDialog()}>
        <ConfirmActionDialog
          title={plural(bulkTargets().length, { one: "admin.users.bulkTitle.one", other: "admin.users.bulkTitle.other" })}
          message={
            <div class="space-y-2">
              <p>{t("admin.users.bulkBody")}</p>
              <ul class="max-h-40 overflow-y-auto list-disc pl-5" data-testid="bulk-ban-names">
                <For each={bulkTargets()}>
                  {(u) => <li>{nameOf(u)} (@{u.username})</li>}
                </For>
              </ul>
            </div>
          }
          confirmLabel={plural(bulkTargets().length, { one: "admin.users.bulkConfirm.one", other: "admin.users.bulkConfirm.other" })}
          danger
          confirmDisabled={!bulkBanReason().trim() || bulkTargets().length === 0}
          failureFallback="admin.action.failed"
          onConfirm={handleBulkBan}
          onDone={() => setShowBulkBanDialog(false)}
          onCancel={() => {
            setShowBulkBanDialog(false);
            setBulkBanReason("");
          }}
          testId="bulk-ban-dialog"
        >
          <label class="block space-y-2">
            <span class="text-sm font-medium text-text-secondary">{t("admin.users.reasonAll")}</span>
            <textarea
              value={bulkBanReason()}
              onInput={(e) => setBulkBanReason(e.currentTarget.value)}
              placeholder={t("admin.users.reasonPlaceholder")}
              rows={3}
              class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary text-sm resize-none"
            />
          </label>
        </ConfirmActionDialog>
      </Show>

      <Show when={bulkFailures().length > 0}>
        <div
          role="alert"
          data-testid="bulk-ban-failures"
          class="fixed bottom-4 left-1/2 -translate-x-1/2 z-40 max-w-md p-3 rounded-lg bg-status-error/15 border border-status-error/50 text-sm text-text-primary shadow-xl"
          style={{ "background-color": "var(--color-surface-layer1)" }}
        >
          <div class="flex items-start gap-3">
            <span>{t("admin.users.bulkFailed", { names: formatList(bulkFailures()) })}</span>
            <button onClick={() => setBulkFailures([])} aria-label={t("common.close")}>
              <X class="w-4 h-4" />
            </button>
          </div>
        </div>
      </Show>

      {/* Delete: the username must be typed. */}
      <Show when={showDeleteDialog() && selectedUser()}>
        {(user) => (
          <ConfirmActionDialog
            title={t("admin.users.deleteTitle", { name: nameOf(user()) })}
            message={t("admin.users.deleteBody", { name: nameOf(user()) })}
            confirmLabel={t("admin.users.deleteConfirm")}
            danger
            confirmDisabled={deleteConfirmText() !== user().username}
            failureFallback="admin.action.failed"
            onConfirm={() => handleDelete(user())}
            onDone={() => setShowDeleteDialog(false)}
            onCancel={() => {
              setShowDeleteDialog(false);
              setDeleteConfirmText("");
            }}
          >
            <label class="block space-y-2">
              <span class="text-sm font-medium text-text-secondary">
                {t("admin.users.typeToConfirm", { username: user().username })}
              </span>
              <input
                type="text"
                value={deleteConfirmText()}
                onInput={(e) => setDeleteConfirmText(e.currentTarget.value)}
                placeholder={user().username}
                class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-status-error text-sm"
              />
            </label>
          </ConfirmActionDialog>
        )}
      </Show>

      <Show when={showCreateAccount()}>
        <CreateAccountModal meId={currentUser()?.id} onClose={() => setShowCreateAccount(false)} />
      </Show>
    </div>
  );
};

export default UsersPanel;
