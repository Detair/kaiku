/**
 * Security Settings
 *
 * Shows E2EE backup status and allows viewing recovery key.
 */

import { Component, createResource, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Check, Eye } from "lucide-solid";

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
    </div>
  );
};

export default SecuritySettings;
