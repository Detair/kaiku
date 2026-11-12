/**
 * Admin Quick Modal
 *
 * Quick access modal for admin functionality with session elevation,
 * stats overview, and navigation to the full admin dashboard.
 */

import {
  createUniqueId,
  Component,
  createSignal,
  createEffect,
  onMount,
  onCleanup,
  Show,
} from "solid-js";
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
import ElevateDialog from "./ElevateDialog";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  deElevateSession,
  getElevationTimeRemaining,
  expireElevationIfDue,
  ADMIN_MODE_MINUTES,
} from "@/stores/admin";
import { t } from "@/i18n";
import Modal from "@/components/ui/Modal";

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
        expireElevationIfDue();
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);

      onCleanup(() => clearInterval(interval));
    }
  });

  const [showElevate, setShowElevate] = createSignal(false);

  // Handle de-elevation
  const handleDeElevate = async () => {
    await deElevateSession();
  };

  // Handle navigation to full dashboard
  const handleOpenDashboard = () => {
    navigate("/admin");
    props.onClose();
  };

  const titleId = createUniqueId();

  return (
    <>
      {/* A modal dialog (A11Y-1); the passkey question opens above it. */}
      <Modal
        onClose={() => props.onClose()}
        labelledBy={titleId}
        backdropClass="fixed inset-0 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
        class="relative rounded-xl border border-white/10 w-[400px] max-w-full shadow-2xl animate-[fadeIn_0.15s_ease-out]"
        style={{ "background-color": "var(--color-surface-layer1)" }}
        testId="admin-quick-dialog"
      >
          {/* Header */}
          <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-9 h-9 rounded-lg bg-accent-primary/20 flex items-center justify-center">
                <Shield class="w-5 h-5 text-text-primary" aria-hidden="true" />
              </div>
              <h2 id={titleId} class="text-lg font-bold text-text-primary">{t("admin.quick.title")}</h2>
            </div>
            <button
              onClick={props.onClose}
              aria-label={t("common.close")}
              class="p-1.5 tap-target text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" aria-hidden="true" />
            </button>
          </div>

          {/* Content */}
          <div class="p-5 space-y-5">
            {/* Session Status */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                {t("admin.mode.name")}
              </h3>

              <Show
                when={adminState.isElevated}
                fallback={
                  /* Not Elevated State */
                  <div class="p-4 rounded-lg bg-white/8 border border-white/15 space-y-3">
                    <div class="flex items-center gap-2 text-text-primary">
                      <Shield class="w-4 h-4" />
                      <span class="text-sm font-medium">{t("admin.mode.offTitle")}</span>
                    </div>
                    <p class="text-sm text-text-secondary">
                      {t("admin.mode.explain", { minutes: ADMIN_MODE_MINUTES })}
                    </p>
                    <button
                      onClick={() => setShowElevate(true)}
                      disabled={adminState.isElevating}
                      class="w-full px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90 state-disabled"
                    >
                      {adminState.isElevating
                        ? t("guildSettings.working")
                        : t("admin.mode.turnOnFor", { minutes: ADMIN_MODE_MINUTES })}
                    </button>
                  </div>
                }
              >
                {/* Elevated State */}
                <div class="p-4 rounded-lg bg-status-warning/15 border border-status-warning/50 space-y-3">
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2 text-text-primary">
                      <ShieldAlert class="w-4 h-4" />
                      <span class="text-sm font-semibold">{t("admin.mode.onTitle")}</span>
                    </div>
                    <span class="text-xs font-medium text-text-primary">
                      {t("admin.mode.left", { time: timeRemaining() })}
                    </span>
                  </div>
                  <button
                    onClick={handleDeElevate}
                    class="w-full px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                  >
                    {t("admin.mode.turnOff")}
                  </button>
                </div>
              </Show>
            </div>

            {/* Quick Stats */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wide">
                {t("admin.dashboard.stats")}
              </h3>

              <div class="grid grid-cols-3 gap-3">
                {/* Users */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Users class="w-5 h-5 mx-auto mb-1 text-text-secondary" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.user_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">{t("admin.dashboard.users")}</div>
                </div>

                {/* Guilds */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Building2 class="w-5 h-5 mx-auto mb-1 text-text-secondary" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.guild_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">{t("admin.dashboard.servers")}</div>
                </div>

                {/* Banned */}
                <div class="p-3 rounded-lg bg-white/8 border border-white/15 text-center">
                  <Ban class="w-5 h-5 mx-auto mb-1 text-status-error" />
                  <div class="text-lg font-bold text-text-primary">
                    {adminState.stats?.banned_count ?? "-"}
                  </div>
                  <div class="text-xs text-text-secondary">{t("admin.dashboard.banned")}</div>
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
              <span>{t("admin.quick.openDashboard")}</span>
              <ExternalLink class="w-4 h-4" />
            </button>
          </div>
      </Modal>
      <Show when={showElevate()}>
        <ElevateDialog onClose={() => setShowElevate(false)} />
      </Show>
    </>
  );
};

export default AdminQuickModal;
