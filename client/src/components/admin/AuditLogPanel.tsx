/**
 * AuditLogPanel - Audit log viewing panel for admin dashboard
 *
 * Provides audit log listing with filtering and pagination.
 * Displays admin actions with actor, target, and timestamp information.
 */

import {
  Component,
  Show,
  For,
  onMount,
  createSignal,
  createMemo,
} from "solid-js";
import {
  Filter,
  ChevronLeft,
  ChevronRight,
  User,
  Building2,
  Shield,
  FileText,
  Calendar,
  X,
} from "lucide-solid";
import {
  adminState,
  loadAuditLog,
  setAuditLogFilters,
  clearAuditLogFilters,
} from "@/stores/admin";
import TableRowSkeleton from "./TableRowSkeleton";

const PAGE_SIZE = 20;

// Action types available for filtering
const ACTION_TYPES = [
  { value: "", label: "All Actions" },
  { value: "admin.users.ban", label: "User Ban" },
  { value: "admin.users.unban", label: "User Unban" },
  { value: "admin.guilds.suspend", label: "Guild Suspend" },
  { value: "admin.guilds.unsuspend", label: "Guild Unsuspend" },
  { value: "admin.session.elevated", label: "Session Elevated" },
  { value: "admin.session.de_elevated", label: "Session De-elevated" },
  { value: "admin.announcements.create", label: "Announcement Created" },
];

const AuditLogPanel: Component = () => {
  const [filterValue, setFilterValue] = createSignal("");
  const [actionType, setActionType] = createSignal("");
  const [fromDate, setFromDate] = createSignal("");
  const [toDate, setToDate] = createSignal("");
  const [showFilters, setShowFilters] = createSignal(false);

  // Load audit log on mount
  onMount(() => {
    loadAuditLog(1);
  });

  // Calculate total pages
  const totalPages = createMemo(
    () => Math.ceil(adminState.auditLogPagination.total / PAGE_SIZE) || 1,
  );

  // Check if any filter is active
  const hasActiveFilters = createMemo(
    () => filterValue().trim() || actionType() || fromDate() || toDate(),
  );

  // Apply all filters
  const applyFilters = () => {
    setAuditLogFilters({
      action: filterValue().trim() || undefined,
      actionType: actionType() || undefined,
      fromDate: fromDate() ? new Date(fromDate()).toISOString() : undefined,
      toDate: toDate()
        ? new Date(toDate() + "T23:59:59").toISOString()
        : undefined,
    });
  };

  // Clear all filters
  const clearAllFilters = () => {
    setFilterValue("");
    setActionType("");
    setFromDate("");
    setToDate("");
    clearAuditLogFilters();
  };

  // Apply filter (alias for applyFilters)
  const applyFilter = () => {
    applyFilters();
  };

  // Handle page navigation
  const goToPage = (page: number) => {
    if (page >= 1 && page <= totalPages()) {
      loadAuditLog(page, adminState.auditLogFilter || undefined);
    }
  };

  // Format date with time for display
  const formatDate = (dateStr: string): string => {
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  // Get icon component based on action prefix
  const getActionIcon = (action: string) => {
    if (action.includes("users")) {
      return User;
    }
    if (action.includes("guilds")) {
      return Building2;
    }
    if (action.includes("session")) {
      return Shield;
    }
    return FileText;
  };

  // Get action color based on action type
  const getActionColor = (action: string): string => {
    if (action.includes("ban") || action.includes("suspend")) {
      return "text-status-error";
    }
    if (action.includes("unban") || action.includes("unsuspend")) {
      return "text-status-success";
    }
    if (action.includes("elevate")) {
      return "text-status-warning";
    }
    return "text-text-secondary";
  };

  // Format action string for display (e.g., "admin.users.ban" -> "Ban User")
  const formatAction = (action: string): string => {
    // Split by dots and get the last parts
    const parts = action.split(".");
    const lastPart = parts[parts.length - 1] || action;
    const targetPart = parts[parts.length - 2] || "";

    // Capitalize first letter and format
    const formattedAction =
      lastPart.charAt(0).toUpperCase() + lastPart.slice(1);

    // Add target context if available
    if (targetPart === "users") {
      return `${formattedAction} User`;
    }
    if (targetPart === "guilds") {
      return `${formattedAction} Guild`;
    }
    if (targetPart === "session") {
      return `Session ${formattedAction}`;
    }

    return formattedAction;
  };

  // Truncate ID for display
  const truncateId = (id: string): string => {
    return id.slice(0, 8) + "...";
  };

  return (
    <div class="flex flex-1 h-full overflow-hidden">
      {/* Audit Log List */}
      <div class="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 class="text-lg font-bold text-text-primary">Audit Log</h2>

          {/* Filter Controls */}
          <div class="flex items-center gap-2">
            {/* Action prefix filter */}
            <div class="relative">
              <Filter class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="text"
                placeholder="Filter by action prefix..."
                value={filterValue()}
                onInput={(e) => setFilterValue(e.currentTarget.value)}
                onKeyPress={(e) => {
                  if (e.key === "Enter") {
                    applyFilter();
                  }
                }}
                class="pl-9 pr-4 py-2 w-48 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm"
              />
            </div>

            {/* Toggle advanced filters */}
            <button
              onClick={() => setShowFilters(!showFilters())}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
              classList={{
                "bg-accent-primary/20 text-accent-primary": showFilters(),
              }}
              title="Advanced filters"
            >
              <Calendar class="w-4 h-4" />
            </button>

            <button
              onClick={applyFilter}
              class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium text-sm transition-colors hover:bg-accent-primary/90"
            >
              Apply
            </button>
            <Show when={hasActiveFilters()}>
              <button
                onClick={clearAllFilters}
                class="p-2 rounded-lg bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
                title="Clear all filters"
              >
                <X class="w-4 h-4" />
              </button>
            </Show>
          </div>
        </div>

        {/* Advanced Filters Panel */}
        <Show when={showFilters()}>
          <div class="px-4 py-3 border-b border-white/10 bg-white/5">
            <div class="flex items-center gap-4 flex-wrap">
              {/* Action Type Dropdown */}
              <div class="flex items-center gap-2">
                <label class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  Action Type
                </label>
                <select
                  value={actionType()}
                  onChange={(e) => setActionType(e.currentTarget.value)}
                  class="px-3 py-1.5 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
                >
                  <For each={ACTION_TYPES}>
                    {(type) => (
                      <option
                        value={type.value}
                        class="bg-[#1a1a2e] text-text-primary"
                      >
                        {type.label}
                      </option>
                    )}
                  </For>
                </select>
              </div>

              {/* Date Range */}
              <div class="flex items-center gap-2">
                <label class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  From
                </label>
                <input
                  type="date"
                  value={fromDate()}
                  onChange={(e) => setFromDate(e.currentTarget.value)}
                  class="px-3 py-1.5 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
                />
              </div>

              <div class="flex items-center gap-2">
                <label class="text-xs font-medium text-text-secondary uppercase tracking-wide">
                  To
                </label>
                <input
                  type="date"
                  value={toDate()}
                  onChange={(e) => setToDate(e.currentTarget.value)}
                  class="px-3 py-1.5 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
                />
              </div>
            </div>
          </div>
        </Show>

        {/* Active Filter Indicator */}
        <Show when={hasActiveFilters()}>
          <div class="px-4 py-2 border-b border-white/10 bg-accent-primary/10">
            <div class="flex items-center gap-2 flex-wrap text-sm">
              <span class="text-text-secondary">Active filters:</span>
              <Show when={adminState.auditLogFilters.action}>
                <span class="px-2 py-0.5 rounded bg-accent-primary/20 text-accent-primary">
                  prefix: {adminState.auditLogFilters.action}
                </span>
              </Show>
              <Show when={adminState.auditLogFilters.actionType}>
                <span class="px-2 py-0.5 rounded bg-accent-primary/20 text-accent-primary">
                  type:{" "}
                  {ACTION_TYPES.find(
                    (t) => t.value === adminState.auditLogFilters.actionType,
                  )?.label || adminState.auditLogFilters.actionType}
                </span>
              </Show>
              <Show when={adminState.auditLogFilters.fromDate}>
                <span class="px-2 py-0.5 rounded bg-accent-primary/20 text-accent-primary">
                  from:{" "}
                  {new Date(
                    adminState.auditLogFilters.fromDate!,
                  ).toLocaleDateString()}
                </span>
              </Show>
              <Show when={adminState.auditLogFilters.toDate}>
                <span class="px-2 py-0.5 rounded bg-accent-primary/20 text-accent-primary">
                  to:{" "}
                  {new Date(
                    adminState.auditLogFilters.toDate!,
                  ).toLocaleDateString()}
                </span>
              </Show>
            </div>
          </div>
        </Show>

        {/* Audit Log Table */}
        <div class="flex-1 overflow-auto">
          {/* Table Header */}
          <div class="grid grid-cols-5 gap-4 px-4 py-3 border-b border-white/10 bg-white/5 text-xs font-medium text-text-secondary uppercase tracking-wide sticky top-0">
            <div>Action</div>
            <div>Actor</div>
            <div>Target</div>
            <div>IP Address</div>
            <div>Time</div>
          </div>

          {/* Loading State */}
          <Show when={adminState.isAuditLogLoading}>
            <TableRowSkeleton columns={5} rows={10} />
          </Show>

          {/* Audit Log Rows */}
          <Show when={!adminState.isAuditLogLoading}>
            <For
              each={adminState.auditLog}
              fallback={
                <div class="flex items-center justify-center py-12">
                  <div class="text-text-secondary">
                    No audit log entries found
                  </div>
                </div>
              }
            >
              {(entry) => {
                const ActionIcon = getActionIcon(entry.action);
                const actionColor = getActionColor(entry.action);

                return (
                  <div class="grid grid-cols-5 gap-4 px-4 py-3 border-b border-white/5 hover:bg-white/5 transition-colors">
                    {/* Action */}
                    <div class="flex items-center gap-2 min-w-0">
                      <ActionIcon
                        class={`w-4 h-4 flex-shrink-0 ${actionColor}`}
                      />
                      <span
                        class={`text-sm font-medium truncate ${actionColor}`}
                      >
                        {formatAction(entry.action)}
                      </span>
                    </div>

                    {/* Actor */}
                    <div class="flex items-center text-sm text-text-primary truncate">
                      {entry.actor_username || truncateId(entry.actor_id)}
                    </div>

                    {/* Target */}
                    <div class="flex items-center text-sm text-text-secondary truncate">
                      <Show
                        when={entry.target_type && entry.target_id}
                        fallback={<span class="text-text-secondary/50">-</span>}
                      >
                        <span class="capitalize">{entry.target_type}</span>
                        <span class="mx-1 text-text-secondary/50">/</span>
                        <span class="font-mono">
                          {truncateId(entry.target_id!)}
                        </span>
                      </Show>
                    </div>

                    {/* IP Address */}
                    <div class="flex items-center text-sm text-text-secondary font-mono truncate">
                      {entry.ip_address || "-"}
                    </div>

                    {/* Time */}
                    <div class="flex items-center text-sm text-text-secondary truncate">
                      {formatDate(entry.created_at)}
                    </div>
                  </div>
                );
              }}
            </For>
          </Show>
        </div>

        {/* Pagination */}
        <div class="flex items-center justify-between px-4 py-3 border-t border-white/10">
          <div class="text-sm text-text-secondary">
            {adminState.auditLogPagination.total} total entries
          </div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => goToPage(adminState.auditLogPagination.page - 1)}
              disabled={adminState.auditLogPagination.page <= 1}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-sm text-text-primary">
              Page {adminState.auditLogPagination.page} of {totalPages()}
            </span>
            <button
              onClick={() => goToPage(adminState.auditLogPagination.page + 1)}
              disabled={adminState.auditLogPagination.page >= totalPages()}
              class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronRight class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default AuditLogPanel;
