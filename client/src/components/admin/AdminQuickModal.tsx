/**
 * Admin Quick Modal
 *
 * Quick access modal for admin functionality with session elevation,
 * stats overview, and navigation to the full admin dashboard.
 */

import {
  Component,
  createSignal,
  createEffect,
  onMount,
  onCleanup,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { useNavigate } from "@solidjs/router";
import {
  X,
  Shield,
  ShieldAlert,
  Users,
  Building2,
  Ban,
  ExternalLink,
} from "lucide-solid";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  elevateSession,
  deElevateSession,
  getElevationTimeRemaining,
} from "@/stores/admin";

interface AdminQuickModalProps {
  onClose: () => void;
}

const AdminQuickModal: Component<AdminQuickModalProps> = (props) => {
  const navigate = useNavigate();
  const [timeRemaining, setTimeRemaining] = createSignal(
    getElevationTimeRemaining(),
  );

  // Load admin status and stats on mount
  onMount(() => {
    checkAdminStatus();
    loadAdminStats();
  });

  // Update time remaining every second when elevated
  createEffect(() => {
    if (adminState.isElevated) {
      const interval = setInterval(() => {
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);

      onCleanup(() => clearInterval(interval));
    }
  });

  // Handle elevation
  const handleElevate = async () => {
    await elevateSession();
  };

  // Handle de-elevation
  const handleDeElevate = async () => {
    await deElevateSession();
  };

  // Handle navigation to full dashboard
  const handleOpenDashboard = () => {
    navigate("/admin");
    props.onClose();
  };

  // Close on escape key
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  // Close on backdrop click
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center"
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        {/* Backdrop */}
        <div
          class="absolute inset-0 bg-black/60 backdrop-blur-sm"
          onClick={handleBackdropClick}
        />

        {/* Modal */}
        <div
          class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style={{"background-color":"var(--color-surface-layer1)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-9 h-9 rounded-lg bg-purple-500/20 flex items-center justify-center">
                <Shield class="w-5 h-5 text-purple-400" />
              </div>
              <h2 class="text-lg font-bold text-text-primary">Admin Panel</h2>
            </div>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Content */}
          <div class="p-5 space-y-5">
            {/* Session Status */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                Session Status
              </h3>

              <Show
                when={adminState.isElevated}
                fallback={
                  /* Not Elevated State */
                  <div class="p-4 rounded-lg bg-white/8 border border-white/15 space-y-3">
                    <div class="flex items-center gap-2 text-text-primary">
                      <Shield class="w-4 h-4" />
                      <span class="text-sm font-medium">Session not elevated</span>
                    </div>
                    <button
                      onClick={handleElevate}
                      disabled={adminState.isElevating}
                      class="w-full px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      {adminState.isElevating
                        ? "Elevating..."
                        : "Elevate Session"}
                    </button>
                  </div>
                }
              >
                {/* Elevated State */}
                <div class="p-4 rounded-lg bg-status-warning/15 border border-status-warning/50 space-y-3">
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2 text-text-primary">
                      <ShieldAlert class="w-4 h-4" />
                      <span class="text-sm font-semibold">Session Elevated</span>
                    </div>
                    <span class="text-xs font-medium text-text-primary/70">
                      {timeRemaining()} remaining
                    </span>
                  </div>
                  <button
                    onClick={handleDeElevate}
                    class="w-full px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                  >
                    De-elevate Session
                  </button>
                </div>
              </Show>
            </div>

            {/* Quick Stats */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                Quick Stats
              </h3>

              <div class="grid grid-cols-3 gap-3">
                {/* Users */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Users class="w-5 h-5 mx-auto mb-1 text-emerald-400" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.user_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">Users</div>
                </div>

                {/* Guilds */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Building2 class="w-5 h-5 mx-auto mb-1 text-blue-400" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.guild_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">Guilds</div>
                </div>

                {/* Banned */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Ban class="w-5 h-5 mx-auto mb-1 text-status-error" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.banned_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">Banned</div>
                </div>
              </div>
            </div>

            {/* Error Display */}
            <Show when={adminState.error}>
              <div class="p-3 rounded-lg bg-status-error/15 border border-status-error/50 text-text-primary text-sm font-medium">
                {adminState.error}
              </div>
            </Show>

            {/* Open Full Dashboard Button */}
            <button
              onClick={handleOpenDashboard}
              class="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg bg-white/8 border border-white/15 text-text-primary font-medium transition-colors hover:bg-white/15"
            >
              <span>Open Full Dashboard</span>
              <ExternalLink class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default AdminQuickModal;
