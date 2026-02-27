/**
 * Security Settings
 *
 * Shows E2EE backup status, MFA (TOTP) management, and clipboard protection settings.
 */

import {
  Component,
  createResource,
  createSignal,
  Show,
  createEffect,
  lazy,
} from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Check,
  Eye,
  Shield,
  Clipboard,
  ShieldCheck,
  ShieldOff,
  RefreshCw,
  KeyRound,
} from "lucide-solid";
import {
  getClipboardSettings,
  updateClipboardSettings,
  type ClipboardSettings,
  type ProtectionLevel,
} from "@/lib/clipboard";
import {
  mfaDisable,
  mfaGenerateBackupCodes,
  mfaBackupCodeCount,
} from "@/lib/tauri";
import type { MfaBackupCodeCountResponse } from "@/lib/tauri";
import { authState } from "@/stores/auth";
import { updateUser } from "@/stores/auth";
import { showToast } from "@/components/ui/Toast";

const MfaSetupModal = lazy(() => import("./MfaSetupModal"));
const BackupCodesDisplay = lazy(() => import("./BackupCodesDisplay"));

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

  // MFA state
  const [showMfaSetup, setShowMfaSetup] = createSignal(false);
  const [showDisableConfirm, setShowDisableConfirm] = createSignal(false);
  const [disableCode, setDisableCode] = createSignal("");
  const [isDisabling, setIsDisabling] = createSignal(false);
  const [disableError, setDisableError] = createSignal("");
  const [backupCodeCount, setBackupCodeCount] =
    createSignal<MfaBackupCodeCountResponse | null>(null);
  const [showRegenCodes, setShowRegenCodes] = createSignal(false);
  const [regenCodes, setRegenCodes] = createSignal<string[]>([]);
  const [isRegenerating, setIsRegenerating] = createSignal(false);

  const isMfaEnabled = () => authState.user?.mfa_enabled ?? false;

  // Fetch backup code count when MFA is enabled
  const fetchBackupCodeCount = async () => {
    if (!isMfaEnabled()) return;
    try {
      const count = await mfaBackupCodeCount();
      setBackupCodeCount(count);
    } catch {
      // Non-critical
    }
  };

  createEffect(() => {
    if (isMfaEnabled()) {
      fetchBackupCodeCount();
    }
  });

  const handleDisableMfa = async (e: Event) => {
    e.preventDefault();
    if (!disableCode().trim()) {
      setDisableError("Enter your MFA code");
      return;
    }

    setIsDisabling(true);
    setDisableError("");
    try {
      await mfaDisable(disableCode());
      updateUser({ mfa_enabled: false });
      setShowDisableConfirm(false);
      setDisableCode("");
      setBackupCodeCount(null);
      showToast({
        type: "success",
        title: "MFA Disabled",
        message: "Two-factor authentication has been disabled.",
        duration: 5000,
      });
    } catch (err) {
      setDisableError(
        err instanceof Error ? err.message : "Invalid code. Please try again.",
      );
    } finally {
      setIsDisabling(false);
    }
  };

  const handleRegenerateBackupCodes = async () => {
    setIsRegenerating(true);
    try {
      const result = await mfaGenerateBackupCodes();
      setRegenCodes(result.codes);
      setShowRegenCodes(true);
      await fetchBackupCodeCount();
      showToast({
        type: "success",
        title: "Codes Regenerated",
        message: "New backup codes generated. Old codes are now invalid.",
        duration: 5000,
      });
    } catch (err) {
      showToast({
        type: "error",
        title: "Regeneration Failed",
        message:
          err instanceof Error
            ? err.message
            : "Could not regenerate backup codes.",
        duration: 8000,
      });
    } finally {
      setIsRegenerating(false);
    }
  };

  const handleMfaSetupComplete = () => {
    updateUser({ mfa_enabled: true });
    fetchBackupCodeCount();
  };

  // Clipboard protection settings
  const [clipboardSettings, setClipboardSettings] =
    createSignal<ClipboardSettings | null>(null);
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
        duration: 8000,
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
        duration: 8000,
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
                Last backup:{" "}
                {formatDate(backupStatus()?.backup_created_at ?? null)}
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
            {backupStatus()?.has_backup ? "View Recovery Key" : "Set Up Backup"}
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

      {/* MFA Section */}
      <div class="pt-6 border-t border-white/10">
        <div class="flex items-center gap-3 mb-4">
          <ShieldCheck class="w-5 h-5 text-text-secondary" />
          <h3 class="text-lg font-semibold text-text-primary">
            Two-Factor Authentication
          </h3>
        </div>

        <div class="bg-surface-base rounded-xl p-4 space-y-4">
          {/* MFA Status */}
          <div class="flex items-start gap-4">
            <div
              class="p-2 rounded-lg"
              classList={{
                "bg-green-500/20": isMfaEnabled(),
                "bg-white/10": !isMfaEnabled(),
              }}
            >
              <Show
                when={isMfaEnabled()}
                fallback={<ShieldOff class="w-6 h-6 text-text-muted" />}
              >
                <ShieldCheck class="w-6 h-6 text-green-400" />
              </Show>
            </div>

            <div class="flex-1">
              <h4 class="font-medium text-text-primary">
                {isMfaEnabled() ? "MFA Enabled" : "MFA Not Enabled"}
              </h4>
              <p class="text-sm text-text-secondary mt-1">
                <Show
                  when={isMfaEnabled()}
                  fallback="Add an extra layer of security to your account with an authenticator app."
                >
                  Your account is protected with TOTP-based two-factor
                  authentication.
                </Show>
              </p>
            </div>
          </div>

          {/* Backup Code Count (if MFA enabled) */}
          <Show when={isMfaEnabled() && backupCodeCount()}>
            <div class="flex items-center justify-between p-3 bg-white/5 rounded-lg">
              <div class="flex items-center gap-2">
                <KeyRound class="w-4 h-4 text-text-secondary" />
                <span class="text-sm text-text-secondary">
                  Backup codes remaining
                </span>
              </div>
              <span
                class="text-sm font-mono font-medium"
                classList={{
                  "text-green-400": (backupCodeCount()?.remaining ?? 0) > 3,
                  "text-yellow-400":
                    (backupCodeCount()?.remaining ?? 0) > 0 &&
                    (backupCodeCount()?.remaining ?? 0) <= 3,
                  "text-red-400": (backupCodeCount()?.remaining ?? 0) === 0,
                }}
              >
                {backupCodeCount()!.remaining} / {backupCodeCount()!.total}
              </span>
            </div>

            <Show when={(backupCodeCount()?.remaining ?? 0) <= 2}>
              <div class="flex items-center gap-3 p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
                <AlertTriangle class="w-4 h-4 text-yellow-400 flex-shrink-0" />
                <p class="text-xs text-yellow-200">
                  {backupCodeCount()?.remaining === 0
                    ? "You have no backup codes left. Regenerate new codes to maintain account recovery access."
                    : "You're running low on backup codes. Consider regenerating new ones."}
                </p>
              </div>
            </Show>
          </Show>

          {/* Actions */}
          <div class="pt-4 border-t border-white/10 flex flex-wrap gap-2">
            <Show when={!isMfaEnabled()}>
              <button
                onClick={() => setShowMfaSetup(true)}
                class="flex items-center gap-2 px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-white text-sm font-medium"
              >
                <ShieldCheck class="w-4 h-4" />
                Enable MFA
              </button>
            </Show>

            <Show when={isMfaEnabled()}>
              <button
                onClick={handleRegenerateBackupCodes}
                disabled={isRegenerating()}
                class="flex items-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary text-sm disabled:opacity-50"
              >
                <RefreshCw
                  class="w-4 h-4"
                  classList={{ "animate-spin": isRegenerating() }}
                />
                {isRegenerating()
                  ? "Regenerating..."
                  : "Regenerate Backup Codes"}
              </button>

              <button
                onClick={() => {
                  setShowDisableConfirm(true);
                  setDisableCode("");
                  setDisableError("");
                }}
                class="flex items-center gap-2 px-4 py-2 bg-red-500/20 hover:bg-red-500/30 rounded-lg transition-colors text-red-400 text-sm"
              >
                <ShieldOff class="w-4 h-4" />
                Disable MFA
              </button>
            </Show>
          </div>
        </div>
      </div>

      {/* MFA Setup Modal */}
      <Show when={showMfaSetup()}>
        <MfaSetupModal
          onClose={() => setShowMfaSetup(false)}
          onComplete={handleMfaSetupComplete}
        />
      </Show>

      {/* Disable MFA Confirmation Modal */}
      <Show when={showDisableConfirm()}>
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          onClick={() => setShowDisableConfirm(false)}
        >
          <div
            class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-sm mx-4 p-6"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 class="text-lg font-semibold text-text-primary mb-2">
              Disable MFA
            </h3>
            <p class="text-sm text-text-secondary mb-4">
              Enter your current MFA code to confirm disabling two-factor
              authentication.
            </p>

            <form onSubmit={handleDisableMfa}>
              <input
                type="text"
                class="input-field font-mono text-center text-lg tracking-widest mb-3"
                placeholder="000000"
                value={disableCode()}
                onInput={(e) =>
                  setDisableCode(e.currentTarget.value.replace(/\s/g, ""))
                }
                disabled={isDisabling()}
                maxLength={20}
                autofocus
                required
              />

              <Show when={disableError()}>
                <div
                  class="p-2 mb-3 rounded-md text-sm"
                  style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
                >
                  {disableError()}
                </div>
              </Show>

              <div class="flex gap-2">
                <button
                  type="button"
                  onClick={() => setShowDisableConfirm(false)}
                  class="flex-1 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary text-sm"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={isDisabling()}
                  class="flex-1 px-4 py-2 bg-red-500/80 hover:bg-red-500 rounded-lg transition-colors text-white text-sm font-medium disabled:opacity-50"
                >
                  {isDisabling() ? "Disabling..." : "Disable"}
                </button>
              </div>
            </form>
          </div>
        </div>
      </Show>

      {/* Regenerated Backup Codes Modal */}
      <Show when={showRegenCodes()}>
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          onClick={() => setShowRegenCodes(false)}
        >
          <div
            class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-lg mx-4 p-6 max-h-[90vh] overflow-y-auto"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 class="text-lg font-semibold text-text-primary mb-4">
              New Backup Codes
            </h3>
            <BackupCodesDisplay codes={regenCodes()} />
            <button
              onClick={() => setShowRegenCodes(false)}
              class="btn-primary w-full mt-4"
            >
              Done
            </button>
          </div>
        </div>
      </Show>

      {/* Clipboard Protection Section */}
      <div class="pt-6 border-t border-white/10">
        <div class="flex items-center gap-3 mb-4">
          <Clipboard class="w-5 h-5 text-text-secondary" />
          <h3 class="text-lg font-semibold text-text-primary">
            Clipboard Protection
          </h3>
        </div>

        <div class="bg-surface-base rounded-xl p-4 space-y-4">
          {/* Protection Level */}
          <div>
            <label class="block text-sm font-medium text-text-primary mb-2">
              Protection Level
            </label>
            <p class="text-xs text-text-secondary mb-3">
              Controls how aggressively sensitive data is cleared from your
              clipboard.
            </p>
            <div class="flex gap-2">
              <button
                onClick={() => handleProtectionLevelChange("minimal")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white":
                    clipboardSettings()?.protection_level === "minimal",
                  "bg-white/10 text-text-secondary hover:bg-white/20":
                    clipboardSettings()?.protection_level !== "minimal",
                }}
              >
                Minimal
              </button>
              <button
                onClick={() => handleProtectionLevelChange("standard")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white":
                    clipboardSettings()?.protection_level === "standard",
                  "bg-white/10 text-text-secondary hover:bg-white/20":
                    clipboardSettings()?.protection_level !== "standard",
                }}
              >
                Standard
              </button>
              <button
                onClick={() => handleProtectionLevelChange("strict")}
                disabled={isSavingClipboard()}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary text-white":
                    clipboardSettings()?.protection_level === "strict",
                  "bg-white/10 text-text-secondary hover:bg-white/20":
                    clipboardSettings()?.protection_level !== "strict",
                }}
              >
                Strict
              </button>
            </div>
            <p class="text-xs text-text-secondary mt-2">
              <Show when={clipboardSettings()?.protection_level === "minimal"}>
                Only critical data (recovery phrases) is auto-cleared after 60
                seconds.
              </Show>
              <Show when={clipboardSettings()?.protection_level === "standard"}>
                Sensitive data (invites, recovery phrases) auto-cleared.
                Recommended.
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
                  Reduces auto-clear timeout to 30 seconds for all sensitive
                  content. Enables clipboard tamper detection.
                </p>
              </div>
              <button
                onClick={() =>
                  handleParanoidModeChange(
                    !clipboardSettings()?.paranoid_mode_enabled,
                  )
                }
                disabled={isSavingClipboard()}
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors disabled:opacity-50"
                classList={{
                  "bg-accent-primary":
                    clipboardSettings()?.paranoid_mode_enabled,
                  "bg-white/20": !clipboardSettings()?.paranoid_mode_enabled,
                }}
              >
                <span
                  class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
                  classList={{
                    "translate-x-6": clipboardSettings()?.paranoid_mode_enabled,
                    "translate-x-1":
                      !clipboardSettings()?.paranoid_mode_enabled,
                  }}
                />
              </button>
            </div>
          </div>

          {/* Info about clipboard protection */}
          <div class="flex items-start gap-3 p-3 bg-accent-primary/10 border border-accent-primary/20 rounded-lg mt-4">
            <Shield class="w-4 h-4 text-accent-primary flex-shrink-0 mt-0.5" />
            <p class="text-xs text-text-secondary">
              Clipboard protection helps prevent clipboard hijacking attacks
              where malware replaces copied addresses or recovery phrases with
              attacker-controlled values.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
};

export default SecuritySettings;
