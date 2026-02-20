/**
 * MFA Setup Modal
 *
 * Step-by-step wizard for enabling TOTP-based MFA:
 * 1. Show QR code + secret for authenticator app
 * 2. Verify first TOTP code → triggers backup code generation
 * 3. Display backup codes with copy/download
 */

import { Component, createSignal, Show, Match, Switch } from "solid-js";
import { X, ShieldCheck, QrCode, KeyRound } from "lucide-solid";
import { mfaSetup, mfaVerify } from "@/lib/tauri";
import type { MfaSetupResponse } from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import BackupCodesDisplay from "./BackupCodesDisplay";
import QRCode from "qrcode";

type Step = "qr" | "verify" | "backup";

interface MfaSetupModalProps {
  onClose: () => void;
  onComplete: () => void;
}

const MfaSetupModal: Component<MfaSetupModalProps> = (props) => {
  const [step, setStep] = createSignal<Step>("qr");
  const [setupData, setSetupData] = createSignal<MfaSetupResponse | null>(null);
  const [verifyCode, setVerifyCode] = createSignal("");
  const [backupCodes, setBackupCodes] = createSignal<string[]>([]);
  const [isLoading, setIsLoading] = createSignal(false);
  const [error, setError] = createSignal("");
  const [qrDataUrl, setQrDataUrl] = createSignal<string | null>(null);

  // Start setup on mount
  const startSetup = async () => {
    setIsLoading(true);
    setError("");
    try {
      const data = await mfaSetup();
      setSetupData(data);
      // Generate QR code client-side to avoid leaking the TOTP secret to external services
      const dataUrl = await QRCode.toDataURL(data.qr_code_url, { width: 200, margin: 1 });
      setQrDataUrl(dataUrl);
      setStep("qr");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  };

  // Call setup immediately
  startSetup();

  const handleVerify = async (e: Event) => {
    e.preventDefault();
    if (!verifyCode().trim()) {
      setError("Enter the 6-digit code from your authenticator app");
      return;
    }

    setIsLoading(true);
    setError("");
    try {
      const result = await mfaVerify(verifyCode());
      if (result.backup_codes && result.backup_codes.length > 0) {
        setBackupCodes(result.backup_codes);
      }
      setStep("backup");
      showToast({
        type: "success",
        title: "MFA Enabled",
        message: "Two-factor authentication has been enabled.",
        duration: 5000,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Invalid code. Please try again.");
    } finally {
      setIsLoading(false);
    }
  };

  const handleComplete = () => {
    props.onComplete();
    props.onClose();
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={props.onClose}>
      <div
        class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-lg mx-4 max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <div class="flex items-center gap-3">
            <ShieldCheck class="w-5 h-5 text-accent-primary" />
            <h2 class="text-lg font-semibold text-text-primary">Set Up Two-Factor Authentication</h2>
          </div>
          <button
            onClick={props.onClose}
            class="p-1.5 hover:bg-white/10 rounded-lg transition-colors text-text-secondary"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="p-6">
          <Switch>
            {/* Step 1: QR Code */}
            <Match when={step() === "qr"}>
              <div class="space-y-4">
                <div class="flex items-center gap-2 text-sm text-text-secondary">
                  <div class="w-6 h-6 rounded-full bg-accent-primary text-white flex items-center justify-center text-xs font-bold">1</div>
                  <span>Scan the QR code with your authenticator app</span>
                </div>

                <Show when={setupData()} fallback={
                  <div class="flex justify-center py-8">
                    <span class="w-8 h-8 border-2 border-white/30 border-t-accent-primary rounded-full animate-spin" />
                  </div>
                }>
                  <div class="flex flex-col items-center gap-4">
                    {/* QR Code - generated client-side to keep TOTP secret local */}
                    <div class="p-4 bg-white rounded-xl">
                      <Show when={qrDataUrl()} fallback={
                        <div class="w-48 h-48 flex items-center justify-center">
                          <span class="w-6 h-6 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
                        </div>
                      }>
                        <img
                          src={qrDataUrl()!}
                          alt="MFA QR Code"
                          class="w-48 h-48"
                        />
                      </Show>
                    </div>

                    <div class="w-full">
                      <p class="text-xs text-text-secondary mb-1">Or enter this secret manually:</p>
                      <div class="flex items-center gap-2">
                        <code class="flex-1 px-3 py-2 bg-white/5 rounded-lg font-mono text-sm text-text-primary select-all break-all">
                          {setupData()!.secret}
                        </code>
                        <button
                          onClick={async () => {
                            await navigator.clipboard.writeText(setupData()!.secret);
                            showToast({ type: "success", title: "Copied", message: "Secret copied to clipboard.", duration: 2000 });
                          }}
                          class="px-3 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-secondary text-xs"
                        >
                          Copy
                        </button>
                      </div>
                    </div>
                  </div>
                </Show>

                <Show when={error()}>
                  <div class="p-3 rounded-md text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
                    {error()}
                  </div>
                </Show>

                <button
                  onClick={() => { setStep("verify"); setError(""); }}
                  class="btn-primary w-full"
                  disabled={!setupData() || isLoading()}
                >
                  Next
                </button>
              </div>
            </Match>

            {/* Step 2: Verify Code */}
            <Match when={step() === "verify"}>
              <form onSubmit={handleVerify} class="space-y-4">
                <div class="flex items-center gap-2 text-sm text-text-secondary">
                  <div class="w-6 h-6 rounded-full bg-accent-primary text-white flex items-center justify-center text-xs font-bold">2</div>
                  <span>Enter the code from your authenticator app</span>
                </div>

                <div class="flex items-center gap-3 p-3 bg-accent-primary/10 border border-accent-primary/20 rounded-lg">
                  <QrCode class="w-5 h-5 text-accent-primary flex-shrink-0" />
                  <p class="text-sm text-text-secondary">
                    Open your authenticator app (Google Authenticator, Authy, etc.) and enter the 6-digit code shown for VoiceChat.
                  </p>
                </div>

                <div>
                  <label class="block text-sm font-medium text-text-secondary mb-1">
                    Verification Code
                  </label>
                  <input
                    type="text"
                    class="input-field font-mono text-center text-lg tracking-widest"
                    placeholder="000000"
                    value={verifyCode()}
                    onInput={(e) => setVerifyCode(e.currentTarget.value.replace(/\s/g, ""))}
                    disabled={isLoading()}
                    maxLength={6}
                    autofocus
                    required
                  />
                </div>

                <Show when={error()}>
                  <div class="p-3 rounded-md text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
                    {error()}
                  </div>
                </Show>

                <div class="flex gap-2">
                  <button
                    type="button"
                    onClick={() => { setStep("qr"); setError(""); }}
                    class="flex-1 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary"
                  >
                    Back
                  </button>
                  <button
                    type="submit"
                    class="flex-1 btn-primary flex items-center justify-center gap-2"
                    disabled={isLoading()}
                  >
                    <Show
                      when={!isLoading()}
                      fallback={
                        <>
                          <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                          Verifying...
                        </>
                      }
                    >
                      Verify & Enable
                    </Show>
                  </button>
                </div>
              </form>
            </Match>

            {/* Step 3: Backup Codes */}
            <Match when={step() === "backup"}>
              <div class="space-y-4">
                <div class="flex items-center gap-2 text-sm text-text-secondary">
                  <div class="w-6 h-6 rounded-full bg-green-500 text-white flex items-center justify-center text-xs font-bold">3</div>
                  <span>Save your backup codes</span>
                </div>

                <div class="flex items-center gap-3 p-3 bg-green-500/10 border border-green-500/30 rounded-lg">
                  <KeyRound class="w-5 h-5 text-green-400 flex-shrink-0" />
                  <p class="text-sm text-green-200">
                    MFA is now enabled! Use these backup codes if you lose access to your authenticator app.
                  </p>
                </div>

                <Show when={backupCodes().length > 0}>
                  <BackupCodesDisplay codes={backupCodes()} />
                </Show>

                <button onClick={handleComplete} class="btn-primary w-full">
                  Done
                </button>
              </div>
            </Match>
          </Switch>
        </div>
      </div>
    </div>
  );
};

export default MfaSetupModal;
