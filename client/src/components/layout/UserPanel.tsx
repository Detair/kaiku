/**
 * UserPanel - User Info at Bottom of Sidebar
 *
 * Shows current user's avatar, name, username, and settings button.
 * Fixed to the bottom of the sidebar with mt-auto.
 *
 * Voice controls are in VoiceIsland (appears when connected to voice).
 */

import {
  Component,
  Show,
  createSignal,
  onMount,
  lazy,
  Suspense,
} from "solid-js";
import { Settings, Shield, LogOut } from "lucide-solid";
import { authState, logout } from "@/stores/auth";
import { adminState, checkAdminStatus } from "@/stores/admin";
import { getUserPresence } from "@/stores/presence";
import Avatar from "@/components/ui/Avatar";
import StatusPicker from "@/components/ui/StatusPicker";
import CustomStatusModal from "@/components/ui/CustomStatusModal";
import { ModalFallback, LazyErrorBoundary } from "@/components/ui/LazyFallback";
import type { CustomStatus } from "@/lib/types";

const SettingsModal = lazy(() => import("@/components/settings/SettingsModal"));
const AdminQuickModal = lazy(
  () => import("@/components/admin/AdminQuickModal"),
);

const UserPanel: Component = () => {
  const user = () => authState.user;
  const [showSettings, setShowSettings] = createSignal(false);
  const [showAdmin, setShowAdmin] = createSignal(false);
  const [showStatusPicker, setShowStatusPicker] = createSignal(false);
  const [showCustomStatusModal, setShowCustomStatusModal] = createSignal(false);

  // Get current custom status from presence store
  const currentCustomStatus = () => {
    const userId = user()?.id;
    if (!userId) return null;
    return getUserPresence(userId)?.customStatus ?? null;
  };

  const handleCustomStatusSave = async (_status: CustomStatus | null) => {
    // No-op: backend does not support custom status yet (PresenceUpdate only handles online/away/busy/offline).
    // Wire this up when a custom_status field is added to the presence system.
  };

  onMount(() => {
    checkAdminStatus();
  });

  return (
    <>
      <div class="mt-auto p-3 bg-surface-base/50 border-t border-white/10 relative">
        <Show when={showStatusPicker()}>
          <div
            class="fixed inset-0 z-40 cursor-default"
            onClick={() => setShowStatusPicker(false)}
          />
          <StatusPicker
            currentStatus={user()?.status || "online"}
            onClose={() => setShowStatusPicker(false)}
            onCustomStatusClick={() => setShowCustomStatusModal(true)}
          />
        </Show>

        <div class="flex items-center gap-3">
          {/* User info - Click to change status */}
          <Show when={user()}>
            <button
              class="flex items-center gap-2.5 flex-1 min-w-0 text-left hover:bg-white/5 p-1 rounded-lg transition-colors -ml-1"
              onClick={() => setShowStatusPicker(!showStatusPicker())}
              title="Change Status"
            >
              <Avatar
                src={user()!.avatar_url}
                alt={user()!.display_name}
                size="sm"
                status={user()!.status}
                showStatus
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm font-semibold text-text-primary truncate">
                  {user()!.display_name}
                </div>
                <div class="text-xs text-text-secondary truncate">
                  @{user()!.username}
                </div>
              </div>
            </button>
          </Show>

          {/* Action buttons */}
          <Show when={adminState.isAdmin}>
            <button
              class="p-1.5 hover:bg-white/10 rounded-lg transition-all duration-200"
              classList={{
                "text-accent-success": adminState.isElevated,
                "text-text-secondary hover:text-accent-primary":
                  !adminState.isElevated,
              }}
              title={
                adminState.isElevated ? "Admin Panel (Elevated)" : "Admin Panel"
              }
              onClick={() => setShowAdmin(true)}
            >
              <Shield class="w-4 h-4" />
            </button>
          </Show>
          <button
            class="p-1.5 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded-lg transition-all duration-200"
            title="User Settings"
            onClick={() => setShowSettings(true)}
          >
            <Settings class="w-4 h-4" />
          </button>
          <button
            class="p-1.5 text-text-secondary hover:text-accent-danger hover:bg-white/10 rounded-lg transition-all duration-200"
            title="Logout"
            onClick={() => logout()}
          >
            <LogOut class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Settings Modal */}
      <Show when={showSettings()}>
        <LazyErrorBoundary name="SettingsModal">
          <Suspense fallback={<ModalFallback />}>
            <SettingsModal onClose={() => setShowSettings(false)} />
          </Suspense>
        </LazyErrorBoundary>
      </Show>

      {/* Admin Quick Modal */}
      <Show when={showAdmin()}>
        <LazyErrorBoundary name="AdminQuickModal">
          <Suspense fallback={<ModalFallback />}>
            <AdminQuickModal onClose={() => setShowAdmin(false)} />
          </Suspense>
        </LazyErrorBoundary>
      </Show>

      {/* Custom Status Modal */}
      <Show when={showCustomStatusModal()}>
        <CustomStatusModal
          currentStatus={currentCustomStatus()}
          onSave={handleCustomStatusSave}
          onClose={() => setShowCustomStatusModal(false)}
        />
      </Show>
    </>
  );
};

export default UserPanel;
