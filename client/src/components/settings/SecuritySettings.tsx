/**
 * Security Settings
 *
 * Shows E2EE backup status (only when E2EE is enabled), MFA (TOTP) management, and clipboard protection settings.
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
import PasskeysSection from "./PasskeysSection";
import PasskeyBrowserSetting from "./PasskeyBrowserSetting";
import { isE2EEEnabled } from "@/lib/featureFlags";
import {
  AlertTriangle,
  Check,
  Eye,
  Shield,
  Clipboard,
  ShieldCheck,
  ShieldOff,
} from "lucide-solid";
import {
  getClipboardSettings,
  updateClipboardSettings,
  type ClipboardSettings,
  type ProtectionLevel,
} from "@/lib/clipboard";
import { mfaDisable } from "@/lib/tauri";
import { authState, isProbation } from "@/stores/auth";
import { updateUser } from "@/stores/auth";
import { showToast } from "@/components/ui/Toast";
import Toggle from "@/components/ui/Toggle";
import { withStepUp } from "@/lib/stepUp";
import { offerSignOutOthers, refreshMeSecurity } from "@/stores/security";
import SecurityOverview from "./security/SecurityOverview";
import KeyDevicesSection from "./security/KeyDevicesSection";
import PasswordSection from "./security/PasswordSection";
import RecoveryCodesSection from "./security/RecoveryCodesSection";
import RecoveryContactSection from "./security/RecoveryContactSection";
import { ReadOnlyNote } from "./security/SectionCard";
import { securityErrorText } from "./security/securityErrors";
// TODO(LP rebase): LP's <PasskeysSection /> mounts above the TOTP section.

const MfaSetupModal = lazy(() => import("./MfaSetupModal"));

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
  const e2eeEnabled = isE2EEEnabled();
  const [backupStatus] = createResource<BackupStatus>(async () => {
    // Encryption backup is hidden while E2EE is off (v1.0); skip the lookup.
    if (!e2eeEnabled) {
      return { has_backup: false, backup_created_at: null, version: null };
    }
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
  const [mfaError, setMfaError] = createSignal<string | null>(null);

  const isMfaEnabled = () => authState.user?.mfa_enabled ?? false;

  // TOTP setup needs the step-up `totp.set` (C27); the modal follows it.
  const handleEnableMfa = async () => {
    setMfaError(null);
    try {
      await withStepUp({ action: "totp.set", target: "", targetLabel: "" }, async () => {
        setShowMfaSetup(true);
      });
    } catch (err) {
      setMfaError(securityErrorText(err, "security.totp.failed"));
    }
  };

  const handleDisableMfa = async (e: Event) => {
    e.preventDefault();
    if (!disableCode().trim()) {
      setDisableError("Enter your MFA code");
      return;
    }

    setIsDisabling(true);
    setDisableError("");
    try {
      const code = disableCode();
      await withStepUp({ action: "totp.remove", target: "", targetLabel: "" }, () => mfaDisable(code));
      updateUser({ mfa_enabled: false });
      setShowDisableConfirm(false);
      setDisableCode("");
      void refreshMeSecurity();
      offerSignOutOthers();
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

  const handleMfaSetupComplete = () => {
    updateUser({ mfa_enabled: true });
    void refreshMeSecurity();
    offerSignOutOthers();
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

      {/* Backup Status Card (E2EE only) */}
      <Show when={e2eeEnabled}>
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

      </Show>

      {/* Warning Banner (if no backup) */}
      <Show when={e2eeEnabled && !backupStatus()?.has_backup && !backupStatus.loading}>
        <div class="flex items-center gap-3 p-4 bg-yellow-500/10 border border-yellow-500/30 rounded-xl">
          <AlertTriangle class="w-5 h-5 text-yellow-400 flex-shrink-0" />
          <p class="text-sm text-yellow-200">
            We recommend setting up a recovery key to protect your encrypted
            messages.
          </p>
        </div>
      </Show>

      {/* How this account signs in (v1.0): overview, phones, password, codes, contact */}
      <SecurityOverview />
      <KeyDevicesSection />
      {/* Passkeys (C29) */}
      <div class="pt-6 border-t border-white/10 space-y-4">
        <PasskeysSection />
        <PasskeyBrowserSetting />
      </div>
      <PasswordSection />

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

          {/* Actions */}
          <Show when={mfaError()}>
            <p role="alert" class="text-sm text-text-primary">{mfaError()}</p>
          </Show>
          <Show when={isProbation()}>
            <ReadOnlyNote />
          </Show>
          <div class="pt-4 border-t border-white/10 flex flex-wrap gap-2" classList={{ hidden: isProbation() }}>
            <Show when={!isMfaEnabled()}>
              <button
                onClick={() => void handleEnableMfa()}
                class="flex items-center gap-2 px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium"
              >
                <ShieldCheck class="w-4 h-4" />
                Enable MFA
              </button>
            </Show>

            <Show when={isMfaEnabled()}>
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
                  style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
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
                  class="flex-1 px-4 py-2 bg-red-500/80 hover:bg-red-500 rounded-lg transition-colors text-white text-sm font-medium state-disabled"
                >
                  {isDisabling() ? "Disabling..." : "Disable"}
                </button>
              </div>
            </form>
          </div>
        </div>
      </Show>

      <RecoveryCodesSection />
      <RecoveryContactSection />

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
                aria-pressed={clipboardSettings()?.protection_level === "minimal"}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:cursor-wait"
                classList={{
                  "bg-accent-primary text-on-accent":
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
                aria-pressed={clipboardSettings()?.protection_level === "standard"}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:cursor-wait"
                classList={{
                  "bg-accent-primary text-on-accent":
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
                aria-pressed={clipboardSettings()?.protection_level === "strict"}
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:cursor-wait"
                classList={{
                  "bg-accent-primary text-on-accent":
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
                <div id="paranoid-mode-label" class="block text-sm font-medium text-text-primary">
                  Paranoid Mode
                </div>
                <p id="paranoid-mode-desc" class="text-xs text-text-secondary mt-1">
                  Reduces auto-clear timeout to 30 seconds for all sensitive
                  content. Enables clipboard tamper detection.
                </p>
              </div>
              <Toggle
                checked={!!clipboardSettings()?.paranoid_mode_enabled}
                onChange={(next) => handleParanoidModeChange(next)}
                disabled={isSavingClipboard()}
                labelledBy="paranoid-mode-label"
                describedBy="paranoid-mode-desc"
              />
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
