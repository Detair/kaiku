/**
 * Security Settings
 *
 * Shows E2EE backup status, recovery key management, and clipboard protection settings.
 */

import { Component, createResource, createSignal, Show, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Check, Eye, Shield, Clipboard } from "lucide-solid";
import {
  getClipboardSettings,
  updateClipboardSettings,
  type ClipboardSettings,
  type ProtectionLevel,
} from "@/lib/clipboard";
import { showToast } from "@/components/ui/Toast";

// Type for backup status (matches Tauri command response)
interface BackupStatus {
  has_backup: boolean;
  backup_created_at: string | null;
  version: number | null;
}

interface SecuritySettingsProps {
  onViewRecoveryKey: () => void;
}

const SecuritySettings: Component<SecuritySettingsProps> = (props) => {
  const [backupStatus] = createResource<BackupStatus>(async () => {
    try {
      return await invoke<BackupStatus>("get_backup_status");
    } catch {
      return { has_backup: false, backup_created_at: null, version: null };
    }
  });

  // Clipboard protection settings
  const [clipboardSettings, setClipboardSettings] = createSignal<ClipboardSettings | null>(null);
  const [isSavingClipboard, setIsSavingClipboard] = createSignal(false);

  // Load clipboard settings on mount
  createEffect(async () => {
    try {
      const settings = await getClipboardSettings();
      setClipboardSettings(settings);
    } catch (err) {
      console.error("Failed to load clipboard settings:", err);
    }
  });

  const handleProtectionLevelChange = async (level: ProtectionLevel) => {
    const current = clipboardSettings();
    if (!current) return;

    setIsSavingClipboard(true);
    try {
      const updated = { ...current, protection_level: level };
      await updateClipboardSettings(updated);
      setClipboardSettings(updated);
      showToast({
        type: "success",
        title: "Clipboard Settings Updated",
        message: `Protection level set to ${level}.`,
        duration: 3000,
      });
    } catch (err) {
      console.error("Failed to update clipboard settings:", err);
      showToast({
        type: "error",
        title: "Update Failed",
        message: "Could not update clipboard settings. Please try again.",
      });
    } finally {
      setIsSavingClipboard(false);
    }
  };

  const handleParanoidModeChange = async (enabled: boolean) => {
    const current = clipboardSettings();
    if (!current) return;

    setIsSavingClipboard(true);
    try {
      const updated = { ...current, paranoid_mode_enabled: enabled };
      await updateClipboardSettings(updated);
      setClipboardSettings(updated);
      showToast({
        type: "success",
        title: "Paranoid Mode Updated",
        message: `Paranoid mode ${enabled ? "enabled" : "disabled"}.`,
        duration: 3000,
      });
    } catch (err) {
      console.error("Failed to update clipboard settings:", err);
      showToast({
        type: "error",
        title: "Update Failed",
        message: "Could not update paranoid mode. Please try again.",
      });
    } finally {
      setIsSavingClipboard(false);
    }
  };

  const formatDate = (dateStr: string | null) => {
    if (!dateStr) return "Never";
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  return (
    <div class="space-y-6">
      <h3 class="text-lg font-semibold text-text-primary">Security</h3>

      {/* Backup Status Card */}
      <div class="bg-surface-base rounded-xl p-4">
        <div class="flex items-start gap-4">
          <div
            class="p-2 rounded-lg"
            classList={{
              "bg-green-500/20": backupStatus()?.has_backup,
              "bg-yellow-500/20": !backupStatus()?.has_backup,
            }}
          >
            <Show
              when={backupStatus()?.has_backup}
              fallback={<AlertTriangle class="w-6 h-6 text-yellow-400" />}
            >
              <Check class="w-6 h-6 text-green-400" />
            </Show>
          </div>

          <div class="flex-1">
            <h4 class="font-medium text-text-primary">
              {backupStatus()?.has_backup
                ? "Backup Active"
                : "Backup Not Set Up"}
            </h4>
            <p class="text-sm text-text-secondary mt-1">
              <Show
                when={backupStatus()?.has_backup}
                fallback="Your encryption keys are not backed up. If you lose all devices, you won't be able to read old messages."
              >
                Last backup: {formatDate(backupStatus()?.backup_created_at ?? null)}
              </Show>
            </p>
          </div>
        </div>

        {/* Actions */}
        <div class="mt-4 pt-4 border-t border-white/10">
          <button
            onClick={props.onViewRecoveryKey}
            class="flex items-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
          >
            <Eye class="w-4 h-4" />
            {backupStatus()?.has_backup
              ? "View Recovery Key"
              : "Set Up Backup"}
          </button>
        </div>
      </div>

      {/* Warning Banner (if no backup) */}
      <Show when={!backupStatus()?.has_backup && !backupStatus.loading}>
        <div class="flex items-center gap-3 p-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl">
          <AlertTriangle class="w-5 h-5 text-yellow-400 flex-shrink-0" />
          <p class="text-sm text-yellow-200">
            We recommend setting up a recovery key to protect your encrypted
            messages.
          </p>
        </div>
      </Show>

      {/* Clipboard Protection Section */}
      <div class="pt-6 border-t border-white/10">
        <div class="flex items-center gap-3 mb-4">
          <Clipboard class="w-5 h-5 text-text-secondary" />
          <h3 class="text-lg font-semibold text-text-primary">Clipboard Protection</h3>
        </div>

        <div class="bg-surface-base rounded-xl p-4 space-y-4">
          {/* Protection Level */}
          <div>
            <label class="block text-sm font-medium text-text-primary mb-2">
              Protection Level
            </label>
            <p class="text-xs text-text-secondary mb-3">
              Controls how aggressively sensitive data is cleared from your clipboard.
            </p>
            <div class="flex gap-2">
              <button
                onClick={() => handleProtectionLevelChange("minimal")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white": clipboardSettings()?.protection_level === "minimal",
                  "bg-white/10 text-text-secondary hover:bg-white/20": clipboardSettings()?.protection_level !== "minimal",
                }}
              >
                Minimal
              </button>
              <button
                onClick={() => handleProtectionLevelChange("standard")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white": clipboardSettings()?.protection_level === "standard",
                  "bg-white/10 text-text-secondary hover:bg-white/20": clipboardSettings()?.protection_level !== "standard",
                }}
              >
                Standard
              </button>
              <button
                onClick={() => handleProtectionLevelChange("strict")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white": clipboardSettings()?.protection_level === "strict",
                  "bg-white/10 text-text-secondary hover:bg-white/20": clipboardSettings()?.protection_level !== "strict",
                }}
              >
                Strict
              </button>
            </div>
            <p class="text-xs text-text-secondary mt-2">
              <Show when={clipboardSettings()?.protection_level === "minimal"}>
                Only critical data (recovery phrases) is auto-cleared after 60 seconds.
              </Show>
              <Show when={clipboardSettings()?.protection_level === "standard"}>
                Sensitive data (invites, recovery phrases) auto-cleared. Recommended.
              </Show>
              <Show when={clipboardSettings()?.protection_level === "strict"}>
                All copied data is auto-cleared, with tamper detection enabled.
              </Show>
            </p>
          </div>

          {/* Paranoid Mode */}
          <div class="pt-4 border-t border-white/10">
            <div class="flex items-center justify-between">
              <div class="flex-1">
                <label class="block text-sm font-medium text-text-primary">
                  Paranoid Mode
                </label>
                <p class="text-xs text-text-secondary mt-1">
                  Reduces auto-clear timeout to 30 seconds for all sensitive content.
                  Enables clipboard tamper detection.
                </p>
              </div>
              <button
                onClick={() => handleParanoidModeChange(!clipboardSettings()?.paranoid_mode_enabled)}
                disabled={isSavingClipboard()}
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary": clipboardSettings()?.paranoid_mode_enabled,
                  "bg-white/20": !clipboardSettings()?.paranoid_mode_enabled,
                }}
              >
                <span
                  class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
                  classList={{
                    "translate-x-6": clipboardSettings()?.paranoid_mode_enabled,
                    "translate-x-1": !clipboardSettings()?.paranoid_mode_enabled,
                  }}
                />
              </button>
            </div>
          </div>

          {/* Info about clipboard protection */}
          <div class="flex items-start gap-3 p-3 bg-accent-primary/10 border border-accent-primary/20 rounded-lg mt-4">
            <Shield class="w-4 h-4 text-accent-primary flex-shrink-0 mt-0.5" />
            <p class="text-xs text-text-secondary">
              Clipboard protection helps prevent clipboard hijacking attacks where malware
              replaces copied addresses or recovery phrases with attacker-controlled values.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
};

export default SecuritySettings;
