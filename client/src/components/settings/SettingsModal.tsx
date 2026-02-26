/**
 * Settings Modal
 *
 * Main settings dialog with tabbed navigation.
 */

import { Component, createSignal, For, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Palette, Volume2, Mic, Shield, Eye, User, Bell, Crosshair } from "lucide-solid";
import { invoke } from "@tauri-apps/api/core";
import { initE2EE } from "@/lib/tauri";
import AccountSettings from "./AccountSettings";
import AppearanceSettings from "./AppearanceSettings";
import NotificationSettings from "./NotificationSettings";
import SecuritySettings from "./SecuritySettings";
import PrivacySettings from "./PrivacySettings";
import FocusSettings from "./FocusSettings";
import RecoveryKeyModal from "./RecoveryKeyModal";

interface SettingsModalProps {
  onClose: () => void;
}

type TabId = "account" | "appearance" | "notifications" | "focus" | "audio" | "voice" | "privacy" | "security";

interface TabDefinition {
  id: TabId;
  label: string;
  icon: typeof Palette;
}

const tabs: TabDefinition[] = [
  { id: "account", label: "My Account", icon: User },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "notifications", label: "Notifications", icon: Bell },
  { id: "focus", label: "Focus", icon: Crosshair },
  { id: "audio", label: "Audio", icon: Volume2 },
  { id: "voice", label: "Voice", icon: Mic },
  { id: "privacy", label: "Privacy", icon: Eye },
  { id: "security", label: "Security", icon: Shield },
];

const SettingsModal: Component<SettingsModalProps> = (props) => {
  const [activeTab, setActiveTab] = createSignal<TabId>("account");
  const [showRecoveryKey, setShowRecoveryKey] = createSignal(false);
  const [recoveryKey, setRecoveryKey] = createSignal<{
    fullKey: string;
    chunks: string[];
  } | null>(null);
  const [backupError, setBackupError] = createSignal<string | null>(null);
  const [isBackingUp, setIsBackingUp] = createSignal(false);

  const handleViewRecoveryKey = async () => {
    setBackupError(null);
    try {
      const key = await invoke<{ full_key: string; chunks: string[] }>(
        "generate_recovery_key"
      );
      setRecoveryKey({ fullKey: key.full_key, chunks: key.chunks });
      setShowRecoveryKey(true);
    } catch (e) {
      console.error("Failed to generate recovery key:", e);
      setBackupError("Failed to generate recovery key. Please try again.");
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
      setBackupError("Failed to create backup. Please try again.");
    } finally {
      setIsBackingUp(false);
    }
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
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div class="border border-white/10 rounded-2xl w-[700px] max-h-[600px] flex flex-col shadow-2xl animate-[fadeIn_0.15s_ease-out]" style="background-color: var(--color-surface-layer1)">
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">Settings</h2>
            <button
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
                      onClick={() => setActiveTab(tab.id)}
                      class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors mb-1"
                      classList={{
                        "bg-accent-primary/20 text-text-primary":
                          activeTab() === tab.id,
                        "text-text-secondary hover:text-text-primary hover:bg-white/5":
                          activeTab() !== tab.id,
                      }}
                    >
                      <Icon class="w-4 h-4" classList={{ "text-accent-primary": activeTab() === tab.id }} />
                      <span class="font-medium">{tab.label}</span>
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
                <div class="text-text-secondary">
                  <h3 class="text-lg font-semibold mb-4 text-text-primary">
                    Audio Settings
                  </h3>
                  <p>Audio device settings coming soon...</p>
                </div>
              </Show>

              <Show when={activeTab() === "voice"}>
                <div class="text-text-secondary">
                  <h3 class="text-lg font-semibold mb-4 text-text-primary">
                    Voice Settings
                  </h3>
                  <p>Voice processing settings coming soon...</p>
                </div>
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
        <Show when={showRecoveryKey() && recoveryKey()}>
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
