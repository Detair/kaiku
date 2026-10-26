/**
 * Settings Modal
 *
 * Main settings dialog with tabbed navigation.
 */

import { Component, createSignal, For, Show } from "solid-js";
import { Portal } from "solid-js/web";
import {
  X,
  Palette,
  Volume2,
  Mic,
  Shield,
  Eye,
  User,
  Bell,
  Crosshair,
} from "lucide-solid";
import { invoke } from "@tauri-apps/api/core";
import { initE2EE } from "@/lib/tauri";
import { isE2EEEnabled } from "@/lib/featureFlags";
import { createDialog } from "@/lib/a11y/dialog";
import AccountSettings from "./AccountSettings";
import AppearanceSettings from "./AppearanceSettings";
import NotificationSettings from "./NotificationSettings";
import SecuritySettings from "./SecuritySettings";
import PrivacySettings from "./PrivacySettings";
import FocusSettings from "./FocusSettings";
import RecoveryKeyModal from "./RecoveryKeyModal";
import AudioSettings from "./AudioSettings";
import VoiceSettings from "./VoiceSettings";
import { t, type MessageKey } from "@/i18n";

interface SettingsModalProps {
  onClose: () => void;
  /** Tab to open on; defaults to Account. */
  initialTab?: TabId;
}

type TabId =
  | "account"
  | "appearance"
  | "notifications"
  | "focus"
  | "audio"
  | "voice"
  | "privacy"
  | "security";

interface TabDefinition {
  id: TabId;
  label: MessageKey;
  icon: typeof Palette;
}

const tabs: TabDefinition[] = [
  { id: "account", label: "settings.tabs.account", icon: User },
  { id: "appearance", label: "settings.tabs.appearance", icon: Palette },
  { id: "notifications", label: "common.notifications", icon: Bell },
  { id: "focus", label: "settings.tabs.focus", icon: Crosshair },
  { id: "audio", label: "settings.tabs.audio", icon: Volume2 },
  { id: "voice", label: "settings.tabs.voice", icon: Mic },
  { id: "privacy", label: "settings.tabs.privacy", icon: Eye },
  { id: "security", label: "settings.tabs.security", icon: Shield },
];

const SettingsModal: Component<SettingsModalProps> = (props) => {
  const dialog = createDialog({ onClose: () => props.onClose() });
  const [activeTab, setActiveTab] = createSignal<TabId>(props.initialTab ?? "account");
  const [showRecoveryKey, setShowRecoveryKey] = createSignal(false);
  const [recoveryKey, setRecoveryKey] = createSignal<{
    fullKey: string;
    chunks: string[];
  } | null>(null);
  const [backupError, setBackupError] = createSignal<string | null>(null);
  const [isBackingUp, setIsBackingUp] = createSignal(false);

  const handleViewRecoveryKey = async () => {
    if (!isE2EEEnabled()) return;
    setBackupError(null);
    try {
      const key = await invoke<{ full_key: string; chunks: string[] }>(
        "generate_recovery_key",
      );
      setRecoveryKey({ fullKey: key.full_key, chunks: key.chunks });
      setShowRecoveryKey(true);
    } catch (e) {
      console.error("Failed to generate recovery key:", e);
      setBackupError(t("settings.recoveryKey.generateFailed"));
    }
  };

  const handleConfirmRecoveryKey = async () => {
    const key = recoveryKey();
    if (!key) return;

    setIsBackingUp(true);
    setBackupError(null);

    try {
      // Initialize E2EE using the recovery key as the encryption key.
      // This derives the Olm account from disk (or creates it) and returns
      // the actual identity keys and prekeys so they can be included in the
      // encrypted backup.
      const initResult = await initE2EE(key.fullKey);
      const backupData = JSON.stringify({
        version: 1,
        created_at: new Date().toISOString(),
        device_id: initResult.device_id,
        identity_key_ed25519: initResult.identity_key_ed25519,
        identity_key_curve25519: initResult.identity_key_curve25519,
        prekeys: initResult.prekeys,
      });
      await invoke("create_backup", {
        recoveryKey: key.fullKey,
        backupData,
      });
      setShowRecoveryKey(false);
      setRecoveryKey(null);
    } catch (e) {
      console.error("Failed to create backup:", e);
      setBackupError(t("settings.recoveryKey.backupFailed"));
    } finally {
      setIsBackingUp(false);
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
        class="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
      >
        <div
          {...dialog.dialogProps}
          ref={dialog.ref}
          class="border border-white/10 rounded-2xl w-[700px] max-h-[600px] flex flex-col shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style={{"background-color":"var(--color-surface-layer1)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <h2 id={dialog.titleId} class="text-xl font-bold text-text-primary">
              {t("settings.modal.title")}
            </h2>
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          <div class="flex flex-1 overflow-hidden">
            {/* Sidebar tabs */}
            <div class="w-48 border-r border-white/10 p-3">
              <For each={tabs}>
                {(tab) => {
                  const Icon = tab.icon;
                  return (
                    <button
                      type="button"
                      data-testid={`settings-tab-${tab.id}`}
                      onClick={() => setActiveTab(tab.id)}
                      class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors mb-1"
                      classList={{
                        "bg-accent-primary/25 text-text-primary":
                          activeTab() === tab.id,
                        "text-text-secondary hover:text-text-primary hover:bg-white/5":
                          activeTab() !== tab.id,
                      }}
                    >
                      <Icon
                        class="w-4 h-4"
                        classList={{
                          "text-accent-primary": activeTab() === tab.id,
                        }}
                      />
                      <span class="font-medium">{t(tab.label)}</span>
                    </button>
                  );
                }}
              </For>
            </div>

            {/* Content area */}
            <div class="flex-1 overflow-y-auto p-6">
              <Show when={activeTab() === "account"}>
                <AccountSettings />
              </Show>

              <Show when={activeTab() === "appearance"}>
                <AppearanceSettings />
              </Show>

              <Show when={activeTab() === "notifications"}>
                <NotificationSettings />
              </Show>

              <Show when={activeTab() === "focus"}>
                <FocusSettings />
              </Show>

              <Show when={activeTab() === "audio"}>
                <AudioSettings />
              </Show>

              <Show when={activeTab() === "voice"}>
                <VoiceSettings />
              </Show>

              <Show when={activeTab() === "privacy"}>
                <PrivacySettings />
              </Show>

              <Show when={activeTab() === "security"}>
                <SecuritySettings onViewRecoveryKey={handleViewRecoveryKey} />
              </Show>
            </div>
          </div>
        </div>

        {/* Recovery Key Modal */}
        <Show when={isE2EEEnabled() && showRecoveryKey() && recoveryKey()}>
          <RecoveryKeyModal
            keyChunks={recoveryKey()!.chunks}
            fullKey={recoveryKey()!.fullKey}
            onConfirm={handleConfirmRecoveryKey}
            onClose={() => {
              setShowRecoveryKey(false);
              setRecoveryKey(null);
              setBackupError(null);
            }}
            error={backupError()}
            isLoading={isBackingUp()}
          />
        </Show>
      </div>
    </Portal>
  );
};

export default SettingsModal;
