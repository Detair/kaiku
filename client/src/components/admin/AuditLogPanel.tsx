/**
 * AuditLogPanel - Audit log viewing panel for admin dashboard
 *
 * Provides audit log listing with filtering and pagination.
 * Displays admin actions with actor, target, and timestamp information.
 */

import { Component, Show, For, onMount, createSignal, createMemo } from "solid-js";
import {
  Filter,
  ChevronLeft,
  ChevronRight,
  User,
  Building2,
  Shield,
  FileText,
} from "lucide-solid";
import { adminState, loadAuditLog } from "@/stores/admin";

const PAGE_SIZE = 20;

const AuditLogPanel: Component = () => {
  const [filterValue, setFilterValue] = createSignal("");

  // Load audit log on mount
  onMount(() => {
    loadAuditLog(1);
  });

  // Calculate total pages
  const totalPages = createMemo(() =>
    Math.ceil(adminState.auditLogPagination.total / PAGE_SIZE) || 1
  );

  // Apply filter
  const applyFilter = () => {
    const filter = filterValue().trim();
    loadAuditLog(1, filter || undefined);
  };

  // Clear filter
  const clearFilter = () => {
    setFilterValue("");
    loadAuditLog(1);
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
    const formattedAction = lastPart.charAt(0).toUpperCase() + lastPart.slice(1);

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

          {/* Filter Input */}
          <div class="flex items-center gap-2">
            <div class="relative">
              <Filter class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="text"
                placeholder="Filter by action..."
                value={filterValue()}
                onInput={(e) => setFilterValue(e.currentTarget.value)}
                onKeyPress={(e) => {
                  if (e.key === "Enter") {
                    applyFilter();
                  }
                }}
                class="pl-9 pr-4 py-2 w-64 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary text-sm"
              />
            </div>
            <button
              onClick={applyFilter}
              class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium text-sm transition-colors hover:bg-accent-primary/90"
            >
              Apply
            </button>
            <Show when={adminState.auditLogFilter}>
              <button
                onClick={clearFilter}
                class="px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium text-sm transition-colors hover:bg-white/20"
              >
                Clear
              </button>
            </Show>
          </div>
        </div>

        {/* Active Filter Indicator */}
        <Show when={adminState.auditLogFilter}>
          <div class="px-4 py-2 border-b border-white/10 bg-accent-primary/10">
            <span class="text-sm text-text-secondary">
              Filtering by:{" "}
              <span class="font-medium text-accent-primary">
                {adminState.auditLogFilter}
              </span>
            </span>
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
            <div class="flex items-center justify-center py-12">
              <div class="text-text-secondary">Loading audit log...</div>
            </div>
          </Show>

          {/* Audit Log Rows */}
          <Show when={!adminState.isAuditLogLoading}>
            <For
              each={adminState.auditLog}
              fallback={
                <div class="flex items-center justify-center py-12">
                  <div class="text-text-secondary">No audit log entries found</div>
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
                      <ActionIcon class={`w-4 h-4 flex-shrink-0 ${actionColor}`} />
                      <span class={`text-sm font-medium truncate ${actionColor}`}>
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
                        <span class="font-mono">{truncateId(entry.target_id!)}</span>
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
