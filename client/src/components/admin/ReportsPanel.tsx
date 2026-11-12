/**
 * ReportsPanel - Report management panel for admin dashboard
 *
 * Provides report listing with filter by status/category, claim, and resolve functionality.
 * Actions require session elevation (two-tier privilege model).
 */

import {
  Component,
  Show,
  For,
  createSignal,
  createMemo,
  createEffect,
} from "solid-js";
import {
  Flag,
  ChevronLeft,
  ChevronRight,
  Loader2,
  UserCheck,
  CheckCircle,
  XCircle,
} from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { adminState } from "@/stores/admin";
import { showToast } from "@/components/ui/Toast";
import Skeleton from "@/components/ui/Skeleton";
import { t, type MessageKey } from "@/i18n";
import Modal from "@/components/ui/Modal";
import Button from "@/components/ui/Button";

const PAGE_SIZE = 20;

const STATUS_COLORS: Record<string, string> = {
  pending: "text-text-primary bg-status-warning/10 border-status-warning/30",
  reviewing: "text-text-primary bg-accent-primary/10 border-accent-primary/30",
  resolved: "text-text-primary bg-status-success/10 border-status-success/30",
  dismissed: "text-text-secondary bg-white/5 border-white/10",
};

const RESOLUTION_ACTIONS: { value: string; label: MessageKey }[] = [
  { value: "dismissed", label: "admin.reports.resolve.dismissed" },
  { value: "warned", label: "admin.reports.resolve.warned" },
  { value: "banned", label: "admin.reports.resolve.banned" },
  { value: "escalated", label: "admin.reports.resolve.escalated" },
];

const ReportsPanel: Component = () => {
  const [reports, setReports] = createSignal<tauri.AdminReportResponse[]>([]);
  const [total, setTotal] = createSignal(0);
  const [page, setPage] = createSignal(1);
  const [isLoading, setIsLoading] = createSignal(false);
  const [statusFilter, setStatusFilter] = createSignal<string>("");
  const [categoryFilter, setCategoryFilter] = createSignal<string>("");
  const [stats, setStats] = createSignal<tauri.ReportStatsResponse | null>(
    null,
  );
  const [statsLoading, setStatsLoading] = createSignal(false);
  const [loadError, setLoadError] = createSignal<string | null>(null);

  // Resolve dialog state
  const [resolveReportId, setResolveReportId] = createSignal<string | null>(
    null,
  );
  const [resolveAction, setResolveAction] = createSignal("dismissed");
  const [resolveNote, setResolveNote] = createSignal("");
  const [actionLoading, setActionLoading] = createSignal(false);

  const totalPages = createMemo(() =>
    Math.max(1, Math.ceil(total() / PAGE_SIZE)),
  );

  const loadReports = async () => {
    setIsLoading(true);
    setLoadError(null);
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
      setLoadError(t("admin.reports.loadFailed"));
      showToast({
        type: "error",
        title: "Failed to load reports",
        duration: 8000,
      });
    } finally {
      setIsLoading(false);
    }
  };

  const loadStats = async () => {
    setStatsLoading(true);
    try {
      const s = await tauri.adminGetReportStats();
      setStats(s);
    } catch (err) {
      console.error("[Admin] Failed to load report stats:", err);
      showToast({
        type: "error",
        title: "Failed to load report stats",
        duration: 8000,
      });
    } finally {
      setStatsLoading(false);
    }
  };

  createEffect(() => {
    if (adminState.isElevated) {
      loadReports();
      loadStats();
    }
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
      showToast({ type: "success", title: "Report claimed", duration: 3000 });
      await loadReports();
      await loadStats();
    } catch (err) {
      showToast({
        type: "error",
        title: "Failed to claim report",
        message: err instanceof Error ? err.message : undefined,
        duration: 8000,
      });
    } finally {
      setActionLoading(false);
    }
  };

  const handleResolve = async () => {
    const id = resolveReportId();
    if (!id) return;

    setActionLoading(true);
    try {
      await tauri.adminResolveReport(
        id,
        resolveAction(),
        resolveNote() || undefined,
      );
      showToast({ type: "success", title: "Report resolved", duration: 3000 });
      setResolveReportId(null);
      setResolveNote("");
      await loadReports();
      await loadStats();
    } catch (err) {
      showToast({
        type: "error",
        title: "Failed to resolve report",
        message: err instanceof Error ? err.message : undefined,
        duration: 8000,
      });
    } finally {
      setActionLoading(false);
    }
  };

  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    return (
      d.toLocaleDateString() +
      " " +
      d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    );
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
          <Show
            when={!statsLoading()}
            fallback={
              <div class="flex items-center gap-2">
                <Skeleton width="90px" height="26px" class="rounded-full" />
                <Skeleton width="100px" height="26px" class="rounded-full" />
              </div>
            }
          >
            <Show when={stats()}>
              <div class="flex items-center gap-2">
                <span class="px-2 py-1 rounded-full text-xs font-medium bg-status-warning/10 text-text-primary border border-status-warning/30">
                  {stats()!.pending} pending
                </span>
                <span class="px-2 py-1 rounded-full text-xs font-medium bg-accent-primary/10 text-text-primary border border-accent-primary/30">
                  {stats()!.reviewing} reviewing
                </span>
              </div>
            </Show>
          </Show>
        </div>

        {/* Filters */}
        <div class="flex gap-3">
          <select
            value={statusFilter()}
            onChange={(e) => {
              setStatusFilter(e.currentTarget.value);
              handleFilterChange();
            }}
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
            onChange={(e) => {
              setCategoryFilter(e.currentTarget.value);
              handleFilterChange();
            }}
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
          <Show when={loadError()}>
            <div class="flex flex-col items-center justify-center p-12 gap-3">
              <p class="text-sm text-status-error">{loadError()}</p>
              <button
                onClick={loadReports}
                class="px-3 py-1.5 text-xs rounded bg-surface-highlight text-text-primary hover:bg-surface-layer2 transition-colors"
              >
                Retry
              </button>
            </div>
          </Show>
          <Show when={!loadError()}>
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
                        <span
                          class={`px-2 py-0.5 rounded-full text-xs font-medium border ${STATUS_COLORS[report.status] ?? "text-text-secondary"}`}
                        >
                          {report.status}
                        </span>
                      </td>
                      <td class="px-4 py-3 text-text-secondary capitalize">
                        {report.category.replace(/_/g, " ")}
                      </td>
                      <td class="px-4 py-3 text-text-primary">
                        <span class="capitalize text-text-secondary">
                          {report.target_type}
                        </span>
                        <br />
                        <span class="text-xs text-text-muted font-mono">
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
                              disabled={
                                !adminState.isElevated || actionLoading()
                              }
                              title="Claim"
                              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-accent-primary/10 transition-colors state-disabled-quiet"
                            >
                              <UserCheck class="w-4 h-4" />
                            </button>
                          </Show>
                          <Show
                            when={
                              report.status === "pending" ||
                              report.status === "reviewing"
                            }
                          >
                            <button
                              onClick={() => {
                                setResolveReportId(report.id);
                                setResolveAction("dismissed");
                              }}
                              disabled={
                                !adminState.isElevated || actionLoading()
                              }
                              title="Resolve"
                              class="p-1.5 rounded-lg text-text-secondary hover:text-status-success hover:bg-status-success/10 transition-colors state-disabled-quiet"
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
        </Show>
      </div>

      {/* Pagination */}
      <Show when={totalPages() > 1}>
        <div class="flex items-center justify-between px-4 py-3 border-t border-white/10">
          <div class="text-xs text-text-secondary">{total()} total reports</div>
          <div class="flex items-center gap-2">
            <button
              onClick={() => handlePageChange(page() - 1)}
              disabled={page() <= 1}
              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors state-disabled-quiet"
            >
              <ChevronLeft class="w-4 h-4" />
            </button>
            <span class="text-xs text-text-secondary">
              Page {page()} of {totalPages()}
            </span>
            <button
              onClick={() => handlePageChange(page() + 1)}
              disabled={page() >= totalPages()}
              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors state-disabled-quiet"
            >
              <ChevronRight class="w-4 h-4" />
            </button>
          </div>
        </div>
      </Show>

      {/* Resolve Dialog */}
      <Show when={resolveReportId()}>
        <Modal
          onClose={() => setResolveReportId(null)}
          labelledBy="admin-resolve-title"
          backdropClass="fixed inset-0 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
          class="relative rounded-xl border border-white/10 w-[400px] max-w-full shadow-2xl"
          style={{ "background-color": "var(--color-surface-layer1)" }}
          testId="resolve-report-dialog"
        >
            <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
              <h3 id="admin-resolve-title" class="text-lg font-bold text-text-primary">
                {t("admin.reports.resolve.title")}
              </h3>
              <button
                type="button"
                onClick={() => setResolveReportId(null)}
                aria-label={t("common.close")}
                class="p-1.5 tap-target text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
              >
                <XCircle class="w-5 h-5" aria-hidden="true" />
              </button>
            </div>
            <div class="p-5 space-y-4">
              <div class="space-y-2">
                <label for="admin-resolve-action" class="text-sm font-medium text-text-secondary">
                  {t("admin.reports.resolve.action")}
                </label>
                <select
                  id="admin-resolve-action"
                  value={resolveAction()}
                  onChange={(e) => setResolveAction(e.currentTarget.value)}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary"
                >
                  <For each={RESOLUTION_ACTIONS}>
                    {(a) => <option value={a.value}>{t(a.label)}</option>}
                  </For>
                </select>
              </div>

              <div class="space-y-2">
                <label for="admin-resolve-note" class="text-sm font-medium text-text-secondary">
                  {t("admin.reports.resolve.note")}
                </label>
                <textarea
                  id="admin-resolve-note"
                  value={resolveNote()}
                  onInput={(e) => setResolveNote(e.currentTarget.value)}
                  placeholder={t("admin.reports.resolve.notePlaceholder")}
                  class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary resize-none text-sm"
                  rows={3}
                />
              </div>

              <div class="flex gap-3 justify-end">
                <Button variant="secondary" onClick={() => setResolveReportId(null)}>
                  {t("common.cancel")}
                </Button>
                <Button onClick={handleResolve} disabled={actionLoading()}>
                  {actionLoading() ? t("admin.reports.resolve.saving") : t("admin.reports.resolve.submit")}
                </Button>
              </div>
            </div>
        </Modal>
      </Show>
    </div>
  );
};

export default ReportsPanel;
