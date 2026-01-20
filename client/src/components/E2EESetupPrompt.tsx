/**
 * E2EE Setup Prompt Component
 *
 * Shows a recovery key modal after login/registration if the user
 * doesn't have an E2EE backup configured. The modal behavior depends
 * on server settings:
 * - If require_e2ee_setup is true: Modal cannot be skipped
 * - If require_e2ee_setup is false: Modal can be skipped
 */

import { Component, createSignal, createResource, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import RecoveryKeyModal from "./settings/RecoveryKeyModal";

// Type for backup status (matches Tauri command response)
interface BackupStatus {
  has_backup: boolean;
  backup_created_at: string | null;
  version: number | null;
}

// Type for server settings (matches Tauri command response)
interface ServerSettings {
  require_e2ee_setup: boolean;
  oidc_enabled: boolean;
}

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Fetch server settings
async function fetchServerSettings(): Promise<ServerSettings> {
  if (isTauri) {
    return invoke<ServerSettings>("get_server_settings");
  }
  // Browser mode - assume not required
  return { require_e2ee_setup: false, oidc_enabled: false };
}

// Fetch backup status
async function fetchBackupStatus(): Promise<BackupStatus> {
  if (isTauri) {
    return invoke<BackupStatus>("get_backup_status");
  }
  // Browser mode - E2EE not available
  return { has_backup: true, backup_created_at: null, version: null };
}

/**
 * E2EE Setup Prompt Component
 *
 * Checks server settings and backup status after login, showing
 * the recovery key setup modal if needed.
 */
const E2EESetupPrompt: Component = () => {
  const [showModal, setShowModal] = createSignal(false);
  const [recoveryKey, setRecoveryKey] = createSignal<{
    fullKey: string;
    chunks: string[];
  } | null>(null);
  const [isRequired, setIsRequired] = createSignal(false);
  const [dismissed, setDismissed] = createSignal(false);

  // Fetch server settings and backup status on mount
  const [serverSettings] = createResource(fetchServerSettings);
  const [backupStatus] = createResource(fetchBackupStatus);

  // Check if we need to show the modal when resources are loaded
  onMount(() => {
    checkAndShowModal();
  });

  // Monitor resource changes
  const checkAndShowModal = async () => {
    // Wait for resources to load
    if (serverSettings.loading || backupStatus.loading) {
      // Re-check when loading completes
      setTimeout(checkAndShowModal, 100);
      return;
    }

    // Skip if already dismissed in this session
    if (dismissed()) return;

    // Skip if there's an error loading data
    if (serverSettings.error || backupStatus.error) {
      console.error("[E2EESetupPrompt] Failed to load settings or backup status");
      return;
    }

    const settings = serverSettings();
    const status = backupStatus();

    // Skip if already has backup
    if (status?.has_backup) return;

    // Skip if not in Tauri (E2EE not available)
    if (!isTauri) return;

    // Show modal
    setIsRequired(settings?.require_e2ee_setup ?? false);

    try {
      const key = await invoke<{ full_key: string; chunks: string[] }>(
        "generate_recovery_key"
      );
      setRecoveryKey({ fullKey: key.full_key, chunks: key.chunks });
      setShowModal(true);
    } catch (e) {
      console.error("[E2EESetupPrompt] Failed to generate recovery key:", e);
    }
  };

  // Handle user confirming they saved the key
  const handleConfirm = async () => {
    const key = recoveryKey();
    if (!key) return;

    try {
      // Create a backup with the key
      const backupData = JSON.stringify({
        version: 1,
        created_at: new Date().toISOString(),
      });
      await invoke("create_backup", {
        recoveryKey: key.fullKey,
        backupData,
      });

      setShowModal(false);
      setRecoveryKey(null);
      setDismissed(true);
    } catch (e) {
      console.error("[E2EESetupPrompt] Failed to create backup:", e);
    }
  };

  // Handle user skipping (only available if not required)
  const handleSkip = () => {
    if (isRequired()) return; // Safety check

    setShowModal(false);
    setRecoveryKey(null);
    setDismissed(true);
  };

  // Handle close attempt (only works if not required)
  const handleClose = () => {
    if (isRequired()) return; // Cannot close if required

    handleSkip();
  };

  return (
    <Show when={showModal() && recoveryKey()}>
      <RecoveryKeyModal
        keyChunks={recoveryKey()!.chunks}
        fullKey={recoveryKey()!.fullKey}
        isInitialSetup={true}
        onConfirm={handleConfirm}
        onSkip={isRequired() ? undefined : handleSkip}
        onClose={handleClose}
      />
    </Show>
  );
};

export default E2EESetupPrompt;
