/**
 * AdminDashboard - Main admin dashboard page
 *
 * Provides the main admin interface with:
 * - Overview panel with stats and quick actions
 * - User management panel
 * - Server management panel
 * - Audit log panel
 *
 * Non-admin users are redirected to home.
 * Admin mode has one name everywhere and says how long it lasts before it is
 * turned on (ADMIN-3); the header badge opens the confirmation directly
 * (ADMIN-12) and turns it off again.
 */

import {
  Component,
  Show,
  createSignal,
  onMount,
  createEffect,
  onCleanup,
} from "solid-js";
import { useNavigate } from "@solidjs/router";
import {
  ArrowLeft,
  Shield,
  ShieldAlert,
  Users,
  Building2,
  Ban,
  X,
} from "lucide-solid";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  getElevationTimeRemaining,
  deElevateSession,
  dismissElevationExpired,
  ADMIN_MODE_MINUTES,
} from "@/stores/admin";
import {
  AdminSidebar,
  UsersPanel,
  GuildsPanel,
  AuditLogPanel,
  ReportsPanel,
  AdminSettings,
  CommandCenterPanel,
  PlatformPagesPanel,
  type AdminPanel,
} from "@/components/admin";
import ElevateDialog from "@/components/admin/ElevateDialog";
import { t } from "@/i18n";

const AdminDashboard: Component = () => {
  const navigate = useNavigate();
  const [activePanel, setActivePanel] = createSignal<AdminPanel>("overview");
  const [timeRemaining, setTimeRemaining] = createSignal<string>("");
  const [showElevate, setShowElevate] = createSignal(false);
  const [statusChecked, setStatusChecked] = createSignal(false);

  // Check admin status and load stats on mount
  onMount(async () => {
    await checkAdminStatus();
    setStatusChecked(true);
    if (adminState.isAdmin) {
      loadAdminStats();
    }
  });

  // Timer to update the admin mode countdown every second
  let timerInterval: ReturnType<typeof setInterval> | null = null;

  createEffect(() => {
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }
    if (adminState.isElevated) {
      setTimeRemaining(getElevationTimeRemaining());
      timerInterval = setInterval(() => {
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);
    } else {
      setTimeRemaining("");
    }
  });

  onCleanup(() => {
    if (timerInterval) {
      clearInterval(timerInterval);
    }
  });

  // Redirect to home if not admin (after loading completes)
  createEffect(() => {
    if (statusChecked() && !adminState.isStatusLoading && !adminState.isAdmin) {
      navigate("/", { replace: true });
    }
  });

  const statCard = (
    Icon: Component<{ class?: string }>,
    label: string,
    value: () => number | undefined,
  ) => (
    <div class="p-4 rounded-xl bg-white/5 border border-white/10">
      <div class="flex items-center gap-3 mb-2">
        <div class="w-10 h-10 rounded-lg bg-accent-primary/20 flex items-center justify-center">
          <Icon class="w-5 h-5 text-text-primary" />
        </div>
        <div class="text-sm text-text-secondary">{label}</div>
      </div>
      <div class="text-2xl font-bold text-text-primary">
        <Show when={!adminState.isStatsLoading && adminState.stats} fallback="…">
          {value() ?? 0}
        </Show>
      </div>
    </div>
  );

  return (
    <div class="h-dvh flex flex-col bg-surface-base">
      {/* Header */}
      <header class="min-h-14 flex-shrink-0 flex flex-wrap items-center justify-between gap-2 px-4 py-2 border-b border-white/10 bg-surface-layer1">
        <div class="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
          >
            <ArrowLeft class="w-4 h-4" />
            {t("admin.dashboard.back")}
          </button>
          <div class="flex items-center gap-2">
            <Shield class="w-5 h-5 text-accent-primary" />
            <h1 class="text-lg font-bold text-text-primary">{t("admin.dashboard.title")}</h1>
          </div>
        </div>

        {/* Admin mode: one name, its length up front, on and off right here. */}
        <div class="flex items-center gap-2">
          <Show
            when={adminState.isElevated}
            fallback={
              <button
                onClick={() => setShowElevate(true)}
                disabled={adminState.isElevating}
                data-testid="admin-mode-on"
                class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-status-warning/20 text-text-primary text-sm font-medium hover:bg-status-warning/30 transition-colors state-disabled"
              >
                <ShieldAlert class="w-4 h-4" />
                {t("admin.mode.turnOnFor", { minutes: ADMIN_MODE_MINUTES })}
              </button>
            }
          >
            <div
              role="status"
              data-testid="admin-mode-status"
              class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-status-success/25 text-text-primary text-sm font-medium"
            >
              <Shield class="w-4 h-4" />
              {t("admin.mode.onLeft", { time: timeRemaining() })}
            </div>
            <button
              onClick={() => void deElevateSession()}
              class="px-3 py-1.5 rounded-full bg-white/10 text-text-primary text-sm hover:bg-white/20 transition-colors"
            >
              {t("admin.mode.turnOff")}
            </button>
          </Show>
        </div>
      </header>

      {/* Admin mode ran out by itself: say so instead of greying buttons silently. */}
      <Show when={adminState.elevationExpired && !adminState.isElevated}>
        <div
          role="alert"
          data-testid="admin-mode-expired"
          class="flex flex-wrap items-center gap-3 px-4 py-2 bg-status-warning/20 border-b border-status-warning/50 text-sm text-text-primary"
        >
          <span class="flex-1">{t("admin.mode.ended")}</span>
          <button
            onClick={() => setShowElevate(true)}
            class="px-3 py-1 rounded-lg bg-white/10 hover:bg-white/20"
          >
            {t("admin.mode.turnOnAgain")}
          </button>
          <button onClick={dismissElevationExpired} aria-label={t("common.close")} class="p-1">
            <X class="w-4 h-4" />
          </button>
        </div>
      </Show>

      {/* Main Content */}
      <main class="flex-1 flex overflow-hidden">
        <Show when={adminState.isStatusLoading || !statusChecked()}>
          <div class="flex-1 flex items-center justify-center">
            <div class="text-text-secondary">{t("common.loading")}</div>
          </div>
        </Show>

        <Show when={statusChecked() && !adminState.isStatusLoading && adminState.isAdmin}>
          <AdminSidebar activePanel={activePanel()} onSelectPanel={setActivePanel} />

          <div class="flex-1 flex flex-col overflow-hidden bg-surface-layer1">
            <Show when={activePanel() === "overview"}>
              <div class="flex-1 p-6 overflow-auto">
                <div class="max-w-4xl mx-auto space-y-6">
                  <section>
                    <h2 class="text-lg font-bold text-text-primary mb-4">
                      {t("admin.dashboard.stats")}
                    </h2>
                    <div class="grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))] gap-4">
                      {statCard(Users, t("admin.dashboard.users"), () => adminState.stats?.user_count)}
                      {statCard(Building2, t("admin.dashboard.servers"), () => adminState.stats?.guild_count)}
                      {statCard(Ban, t("admin.dashboard.banned"), () => adminState.stats?.banned_count)}
                    </div>
                  </section>

                  <section>
                    <h2 class="text-lg font-bold text-text-primary mb-4">
                      {t("admin.dashboard.shortcuts")}
                    </h2>
                    <div class="flex flex-wrap gap-3">
                      <button
                        onClick={() => setActivePanel("users")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90"
                      >
                        <Users class="w-4 h-4" />
                        {t("admin.dashboard.manageUsers")}
                      </button>
                      <button
                        onClick={() => setActivePanel("guilds")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                      >
                        <Building2 class="w-4 h-4" />
                        {t("admin.dashboard.manageServers")}
                      </button>
                      <button
                        onClick={() => setActivePanel("audit-log")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                      >
                        <Shield class="w-4 h-4" />
                        {t("admin.dashboard.auditLog")}
                      </button>
                    </div>
                  </section>

                  <Show when={!adminState.isElevated}>
                    <section>
                      <div class="p-4 rounded-xl bg-status-warning/20 border border-status-warning/50">
                        <div class="flex items-start gap-3">
                          <ShieldAlert class="w-5 h-5 text-status-warning flex-shrink-0 mt-0.5" />
                          <div class="space-y-2">
                            <h3 class="text-sm font-semibold text-text-primary">
                              {t("admin.mode.offTitle")}
                            </h3>
                            <p class="text-sm text-text-primary">
                              {t("admin.mode.explain", { minutes: ADMIN_MODE_MINUTES })}
                            </p>
                            <button
                              onClick={() => setShowElevate(true)}
                              class="px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90"
                            >
                              {t("admin.mode.turnOnFor", { minutes: ADMIN_MODE_MINUTES })}
                            </button>
                          </div>
                        </div>
                      </div>
                    </section>
                  </Show>
                </div>
              </div>
            </Show>

            <Show when={activePanel() === "users"}>
              <UsersPanel />
            </Show>
            <Show when={activePanel() === "guilds"}>
              <GuildsPanel />
            </Show>
            <Show when={activePanel() === "platform-pages"}>
              <PlatformPagesPanel />
            </Show>
            <Show when={activePanel() === "reports"}>
              <ReportsPanel />
            </Show>
            <Show when={activePanel() === "audit-log"}>
              <AuditLogPanel />
            </Show>
            <Show when={activePanel() === "command-center"}>
              <CommandCenterPanel />
            </Show>
            <Show when={activePanel() === "settings"}>
              <AdminSettings />
            </Show>
          </div>
        </Show>
      </main>

      {/* The badge opens the passkey confirmation directly (ADMIN-12). */}
      <Show when={showElevate()}>
        <ElevateDialog onClose={() => setShowElevate(false)} />
      </Show>
    </div>
  );
};

export default AdminDashboard;
