/**
 * ReportsPanel - Report management panel for admin dashboard
 *
 * Provides report listing with filter by status/category, claim, and resolve functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import { Component, Show, For, onMount, createSignal, createMemo } from "solid-js";
import { Flag, ChevronLeft, ChevronRight, Loader2, UserCheck, CheckCircle, XCircle } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { adminState } from "@/stores/admin";
import { showToast } from "@/components/ui/Toast";

const PAGE_SIZE = 20;

const STATUS_COLORS: Record<string, string> = {
  pending: "text-status-warning bg-status-warning/10 border-status-warning/30",
  reviewing: "text-blue-400 bg-blue-400/10 border-blue-400/30",
  resolved: "text-status-success bg-status-success/10 border-status-success/30",
  dismissed: "text-text-secondary bg-white/5 border-white/10",
};

const RESOLUTION_ACTIONS = [
  { value: "dismissed", label: "Dismiss" },
  { value: "warned", label: "Warn User" },
  { value: "banned", label: "Ban User" },
  { value: "escalated", label: "Escalate" },
];

const ReportsPanel: Component = () => {
  const [reports, setReports] = createSignal<tauri.AdminReportResponse[]>([]);
  const [total, setTotal] = createSignal(0);
  const [page, setPage] = createSignal(1);
  const [isLoading, setIsLoading] = createSignal(false);
  const [statusFilter, setStatusFilter] = createSignal<string>("");
  const [categoryFilter, setCategoryFilter] = createSignal<string>("");
  const [stats, setStats] = createSignal<tauri.ReportStatsResponse | null>(null);

  // Resolve dialog state
  const [resolveReportId, setResolveReportId] = createSignal<string | null>(null);
  const [resolveAction, setResolveAction] = createSignal("dismissed");
  const [resolveNote, setResolveNote] = createSignal("");
  const [actionLoading, setActionLoading] = createSignal(false);

  const totalPages = createMemo(() => Math.max(1, Math.ceil(total() / PAGE_SIZE)));

  const loadReports = async () => {
    setIsLoading(true);
    try {
      const offset = (page() - 1) * PAGE_SIZE;
      const result = await tauri.adminListReports(
        PAGE_SIZE,
        offset,
        statusFilter() || undefined,
        categoryFilter() || undefined,
      );
      setReports(result.items);
      setTotal(result.total);
    } catch (err) {
      console.error("[Admin] Failed to load reports:", err);
    } finally {
      setIsLoading(false);
    }
  };

  const loadStats = async () => {
    try {
      const s = await tauri.adminGetReportStats();
      setStats(s);
    } catch (err) {
      console.error("[Admin] Failed to load report stats:", err);
    }
  };

  onMount(() => {
    loadReports();
    loadStats();
  });

  const handlePageChange = (newPage: number) => {
    setPage(newPage);
    loadReports();
  };

  const handleFilterChange = () => {
    setPage(1);
    loadReports();
  };

  const handleClaim = async (reportId: string) => {
    setActionLoading(true);
    try {
      await tauri.adminClaimReport(reportId);
      showToast({ type: "success", title: "Report claimed" });
      await loadReports();
      await loadStats();
    } catch (err) {
      showToast({ type: "error", title: "Failed to claim report", message: err instanceof Error ? err.message : undefined });
    } finally {
      setActionLoading(false);
    }
  };

  const handleResolve = async () => {
    const id = resolveReportId();
    if (!id) return;

    setActionLoading(true);
    try {
      await tauri.adminResolveReport(id, resolveAction(), resolveNote() || undefined);
      showToast({ type: "success", title: "Report resolved" });
      setResolveReportId(null);
      setResolveNote("");
      await loadReports();
      await loadStats();
    } catch (err) {
      showToast({ type: "error", title: "Failed to resolve report", message: err instanceof Error ? err.message : undefined });
    } finally {
      setActionLoading(false);
    }
  };

  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    return d.toLocaleDateString() + " " + d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  return (
    <div class="flex-1 flex flex-col overflow-hidden">
      {/* Header with stats */}
      <div class="p-4 border-b border-white/10 space-y-3">
        <div class="flex items-center justify-between">
          <h2 class="text-lg font-bold text-text-primary flex items-center gap-2">
            <Flag class="w-5 h-5" />
            Reports
          </h2>

          {/* Stats badges */}
          <Show when={stats()}>
            <div class="flex items-center gap-2">
              <span class="px-2 py-1 rounded-full text-xs font-medium bg-status-warning/10 text-status-warning border border-status-warning/30">
                {stats()!.pending} pending
              </span>
              <span class="px-2 py-1 rounded-full text-xs font-medium bg-blue-400/10 text-blue-400 border border-blue-400/30">
                {stats()!.reviewing} reviewing
              </span>
            </div>
          </Show>
        </div>

        {/* Filters */}
        <div class="flex gap-3">
          <select
            value={statusFilter()}
            onChange={(e) => { setStatusFilter(e.currentTarget.value); handleFilterChange(); }}
            class="px-3 py-1.5 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
          >
            <option value="">All Statuses</option>
            <option value="pending">Pending</option>
            <option value="reviewing">Reviewing</option>
            <option value="resolved">Resolved</option>
            <option value="dismissed">Dismissed</option>
          </select>

          <select
            value={categoryFilter()}
            onChange={(e) => { setCategoryFilter(e.currentTarget.value); handleFilterChange(); }}
            class="px-3 py-1.5 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
          >
            <option value="">All Categories</option>
            <option value="harassment">Harassment</option>
            <option value="spam">Spam</option>
            <option value="inappropriate_content">Inappropriate Content</option>
            <option value="impersonation">Impersonation</option>
            <option value="other">Other</option>
          </select>
        </div>
      </div>

      {/* Table */}
      <div class="flex-1 overflow-auto">
        <Show
          when={!isLoading()}
          fallback={
            <div class="flex items-center justify-center p-12">
              <Loader2 class="w-6 h-6 text-text-secondary animate-spin" />
            </div>
          }
        >
          <Show
            when={reports().length > 0}
            fallback={
              <div class="flex items-center justify-center p-12 text-text-secondary text-sm">
                No reports found.
              </div>
            }
          >
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-white/10 text-text-secondary text-xs uppercase tracking-wide">
                  <th class="px-4 py-3 text-left font-medium">Status</th>
                  <th class="px-4 py-3 text-left font-medium">Category</th>
                  <th class="px-4 py-3 text-left font-medium">Target</th>
                  <th class="px-4 py-3 text-left font-medium">Description</th>
                  <th class="px-4 py-3 text-left font-medium">Created</th>
                  <th class="px-4 py-3 text-right font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                <For each={reports()}>
                  {(report) => (
                    <tr class="border-b border-white/5 hover:bg-white/3 transition-colors">
                      <td class="px-4 py-3">
                        <span class={`px-2 py-0.5 rounded-full text-xs font-medium border ${STATUS_COLORS[report.status] ?? "text-text-secondary"}`}>
                          {report.status}
                        </span>
                      </td>
                      <td class="px-4 py-3 text-text-secondary capitalize">
                        {report.category.replace(/_/g, " ")}
                      </td>
                      <td class="px-4 py-3 text-text-primary">
                        <span class="capitalize text-text-secondary">{report.target_type}</span>
                        <br />
                        <span class="text-xs text-text-secondary/50 font-mono">
                          {report.target_user_id.slice(0, 8)}...
                        </span>
                      </td>
                      <td class="px-4 py-3 text-text-secondary max-w-xs truncate">
                        {report.description ?? "-"}
                      </td>
                      <td class="px-4 py-3 text-text-secondary text-xs">
                        {formatDate(report.created_at)}
                      </td>
                      <td class="px-4 py-3 text-right">
                        <div class="flex items-center justify-end gap-1">
                          <Show when={report.status === "pending"}>
                            <button
                              onClick={() => handleClaim(report.id)}
                              disabled={!adminState.isElevated || actionLoading()}
                              title="Claim"
                              class="p-1.5 rounded-lg text-text-secondary hover:text-blue-400 hover:bg-blue-400/10 transition-colors disabled:opacity-30"
                            >
                              <UserCheck class="w-4 h-4" />
                            </button>
                          </Show>
                          <Show when={report.status === "pending" || report.status === "reviewing"}>
                            <button
                              onClick={() => { setResolveReportId(report.id); setResolveAction("dismissed"); }}
                              disabled={!adminState.isElevated || actionLoading()}
                              title="Resolve"
                              class="p-1.5 rounded-lg text-text-secondary hover:text-status-success hover:bg-status-success/10 transition-colors disabled:opacity-30"
                            >
                              <CheckCircle class="w-4 h-4" />
                            </button>
                          </Show>
                        </div>
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </Show>
        </Show>
      </div>

      {/* Pagination */}
      <Show when={totalPages() > 1}>
        <div class="flex items-center justify-between px-4 py-3 border-t border-white/10">
          <div class="text-xs text-text-secondary">
            {total()} total reports
          </div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => handlePageChange(page() - 1)}
              disabled={page() <= 1}
              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-30"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-xs text-text-secondary">
              Page {page()} of {totalPages()}
            </span>
            <button
              onClick={() => handlePageChange(page() + 1)}
              disabled={page() >= totalPages()}
              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors disabled:opacity-30"
            >
              <ChevronRight class="w-4 h-4" />
            </button>
          </div>
        </div>
      </Show>

      {/* Resolve Dialog */}
      <Show when={resolveReportId()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          <div
            class="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setResolveReportId(null)}
          />
          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
              <h3 class="text-lg font-bold text-text-primary">Resolve Report</h3>
              <button
                onClick={() => setResolveReportId(null)}
                class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
              >
                <XCircle class="w-5 h-5" />
              </button>
            </div>
            <div class="p-5 space-y-4">
              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">Action</label>
                <select
                  value={resolveAction()}
                  onChange={(e) => setResolveAction(e.currentTarget.value)}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
                >
                  <For each={RESOLUTION_ACTIONS}>
                    {(a) => <option value={a.value}>{a.label}</option>}
                  </For>
                </select>
              </div>

              <div class="space-y-2">
                <label class="text-sm font-medium text-text-secondary">Note (optional)</label>
                <textarea
                  value={resolveNote()}
                  onInput={(e) => setResolveNote(e.currentTarget.value)}
                  placeholder="Resolution notes..."
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary resize-none text-sm"
                  rows={3}
                />
              </div>

              <div class="flex gap-3 justify-end">
                <button
                  onClick={() => setResolveReportId(null)}
                  class="px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                >
                  Cancel
                </button>
                <button
                  onClick={handleResolve}
                  disabled={actionLoading()}
                  class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium transition-colors hover:bg-accent-primary/90 disabled:opacity-50"
                >
                  {actionLoading() ? "Resolving..." : "Resolve"}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ReportsPanel;
