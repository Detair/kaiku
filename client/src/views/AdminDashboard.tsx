/**
 * AdminDashboard - Main admin dashboard page
 *
 * Provides the main admin interface with:
 * - Overview panel with stats and quick actions
 * - User management panel
 * - Guild management panel
 * - Audit log panel
 *
 * Non-admin users are redirected to home.
 * Session elevation status is displayed with countdown timer.
 */

import { Component, Show, createSignal, onMount, createEffect, onCleanup } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { ArrowLeft, Shield, ShieldAlert, Users, Building2, Ban } from "lucide-solid";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  getElevationTimeRemaining,
} from "@/stores/admin";
import {
  AdminSidebar,
  UsersPanel,
  GuildsPanel,
  AuditLogPanel,
  ReportsPanel,
  AdminSettings,
  type AdminPanel,
} from "@/components/admin";

const AdminDashboard: Component = () => {
  const navigate = useNavigate();
  const [activePanel, setActivePanel] = createSignal<AdminPanel>("overview");
  const [timeRemaining, setTimeRemaining] = createSignal<string>("");

  // Check admin status and load stats on mount
  onMount(async () => {
    await checkAdminStatus();
    if (adminState.isAdmin) {
      loadAdminStats();
    }
  });

  // Timer to update elevation countdown every second
  let timerInterval: ReturnType<typeof setInterval> | null = null;

  createEffect(() => {
    // Always clear previous interval first to prevent race conditions
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }

    // Start timer when elevated
    if (adminState.isElevated) {
      // Update immediately
      setTimeRemaining(getElevationTimeRemaining());

      // Then update every second
      timerInterval = setInterval(() => {
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);
    } else {
      setTimeRemaining("");
    }
  });

  // Cleanup timer on unmount
  onCleanup(() => {
    if (timerInterval) {
      clearInterval(timerInterval);
    }
  });

  // Redirect to home if not admin (after loading completes)
  createEffect(() => {
    if (!adminState.isStatusLoading && !adminState.isAdmin) {
      navigate("/");
    }
  });

  // Handle panel selection
  const handleSelectPanel = (panel: AdminPanel) => {
    setActivePanel(panel);
  };

  // Navigate back to app
  const handleBackToApp = () => {
    navigate("/");
  };

  return (
    <div class="h-screen flex flex-col bg-surface-base">
      {/* Header */}
      <header class="h-14 flex-shrink-0 flex items-center justify-between px-4 border-b border-white/10 bg-surface-layer1">
        {/* Left: Back button and title */}
        <div class="flex items-center gap-4">
          <button
            onClick={handleBackToApp}
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
          >
            <ArrowLeft class="w-4 h-4" />
            Back to App
          </button>
          <div class="flex items-center gap-2">
            <Shield class="w-5 h-5 text-accent-primary" />
            <h1 class="text-lg font-bold text-text-primary">Admin Dashboard</h1>
          </div>
        </div>

        {/* Right: Elevation status */}
        <div class="flex items-center">
          <Show
            when={adminState.isElevated}
            fallback={
              <div class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/10 text-text-secondary text-sm">
                <ShieldAlert class="w-4 h-4" />
                Not Elevated
              </div>
            }
          >
            <div class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-status-success/20 text-status-success text-sm font-medium">
              <Shield class="w-4 h-4" />
              Elevated ({timeRemaining()})
            </div>
          </Show>
        </div>
      </header>

      {/* Main Content */}
      <main class="flex-1 flex overflow-hidden">
        {/* Loading State */}
        <Show when={adminState.isStatusLoading}>
          <div class="flex-1 flex items-center justify-center">
            <div class="text-text-secondary">Loading admin status...</div>
          </div>
        </Show>

        {/* Admin Content */}
        <Show when={!adminState.isStatusLoading && adminState.isAdmin}>
          {/* Sidebar */}
          <AdminSidebar
            activePanel={activePanel()}
            onSelectPanel={handleSelectPanel}
          />

          {/* Panel Content */}
          <div class="flex-1 flex flex-col overflow-hidden bg-surface-layer1">
            {/* Overview Panel */}
            <Show when={activePanel() === "overview"}>
              <div class="flex-1 p-6 overflow-auto">
                <div class="max-w-4xl mx-auto space-y-6">
                  {/* Quick Stats */}
                  <section>
                    <h2 class="text-lg font-bold text-text-primary mb-4">Quick Stats</h2>
                    <div class="grid grid-cols-3 gap-4">
                      {/* Users Card */}
                      <div class="p-4 rounded-xl bg-white/5 border border-white/10">
                        <div class="flex items-center gap-3 mb-2">
                          <div class="w-10 h-10 rounded-lg bg-emerald-500/20 flex items-center justify-center">
                            <Users class="w-5 h-5 text-emerald-400" />
                          </div>
                          <div class="text-sm text-text-secondary">Users</div>
                        </div>
                        <div class="text-2xl font-bold text-text-primary">
                          <Show
                            when={!adminState.isStatsLoading && adminState.stats}
                            fallback="..."
                          >
                            {adminState.stats?.user_count ?? 0}
                          </Show>
                        </div>
                      </div>

                      {/* Guilds Card */}
                      <div class="p-4 rounded-xl bg-white/5 border border-white/10">
                        <div class="flex items-center gap-3 mb-2">
                          <div class="w-10 h-10 rounded-lg bg-blue-500/20 flex items-center justify-center">
                            <Building2 class="w-5 h-5 text-blue-400" />
                          </div>
                          <div class="text-sm text-text-secondary">Guilds</div>
                        </div>
                        <div class="text-2xl font-bold text-text-primary">
                          <Show
                            when={!adminState.isStatsLoading && adminState.stats}
                            fallback="..."
                          >
                            {adminState.stats?.guild_count ?? 0}
                          </Show>
                        </div>
                      </div>

                      {/* Banned Card */}
                      <div class="p-4 rounded-xl bg-white/5 border border-white/10">
                        <div class="flex items-center gap-3 mb-2">
                          <div class="w-10 h-10 rounded-lg bg-status-error/20 flex items-center justify-center">
                            <Ban class="w-5 h-5 text-status-error" />
                          </div>
                          <div class="text-sm text-text-secondary">Banned</div>
                        </div>
                        <div class="text-2xl font-bold text-text-primary">
                          <Show
                            when={!adminState.isStatsLoading && adminState.stats}
                            fallback="..."
                          >
                            {adminState.stats?.banned_count ?? 0}
                          </Show>
                        </div>
                      </div>
                    </div>
                  </section>

                  {/* Quick Actions */}
                  <section>
                    <h2 class="text-lg font-bold text-text-primary mb-4">Quick Actions</h2>
                    <div class="flex gap-3">
                      <button
                        onClick={() => setActivePanel("users")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent-primary text-white font-medium transition-colors hover:bg-accent-primary/90"
                      >
                        <Users class="w-4 h-4" />
                        Manage Users
                      </button>
                      <button
                        onClick={() => setActivePanel("guilds")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                      >
                        <Building2 class="w-4 h-4" />
                        Manage Guilds
                      </button>
                      <button
                        onClick={() => setActivePanel("audit-log")}
                        class="flex items-center gap-2 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                      >
                        <Shield class="w-4 h-4" />
                        View Audit Log
                      </button>
                    </div>
                  </section>

                  {/* Elevation Notice */}
                  <Show when={!adminState.isElevated}>
                    <section>
                      <div class="p-4 rounded-xl bg-status-warning/10 border border-status-warning/30">
                        <div class="flex items-start gap-3">
                          <ShieldAlert class="w-5 h-5 text-status-warning flex-shrink-0 mt-0.5" />
                          <div>
                            <h3 class="text-sm font-medium text-status-warning">
                              Session Not Elevated
                            </h3>
                            <p class="text-sm text-text-secondary mt-1">
                              Some admin actions require session elevation. Use the quick access
                              modal (Ctrl+Shift+A) to elevate your session with MFA verification.
                            </p>
                          </div>
                        </div>
                      </div>
                    </section>
                  </Show>
                </div>
              </div>
            </Show>

            {/* Users Panel */}
            <Show when={activePanel() === "users"}>
              <UsersPanel />
            </Show>

            {/* Guilds Panel */}
            <Show when={activePanel() === "guilds"}>
              <GuildsPanel />
            </Show>

            {/* Reports Panel */}
            <Show when={activePanel() === "reports"}>
              <ReportsPanel />
            </Show>

            {/* Audit Log Panel */}
            <Show when={activePanel() === "audit-log"}>
              <AuditLogPanel />
            </Show>

            {/* Settings Panel */}
            <Show when={activePanel() === "settings"}>
              <AdminSettings />
            </Show>
          </div>
        </Show>
      </main>
    </div>
  );
};

export default AdminDashboard;
