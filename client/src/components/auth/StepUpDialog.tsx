/**
 * StepUpDialog — confirms one sensitive action with a passkey (F7). Names the
 * action and its target, runs the ceremony on the user's click (WebAuthn
 * needs the gesture), and on the desktop says the browser continues. Without
 * a usable passkey it only points to the Kaiku app on the phone.
 * The default export is the host mounted once in `App.tsx`.
 */

import { Component, Show } from "solid-js";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { isTauri } from "@/lib/tauri/common";
import { PasskeyError } from "@/lib/tauri/passkeys";
import { formatPasskeyError } from "@/lib/authErrors";
import { pendingStepUp, stepUpLabel, type PendingStepUp } from "@/lib/stepUp";
import DialogShell from "./DialogShell";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

const STEP_UP_ERRORS = {
  PASSKEY_BROWSER_FAILED: "security.stepUp.error.browserFailed",
  PASSKEY_CANCELLED: "security.stepUp.error.cancelled",
  PASSKEY_TIMEOUT: "security.stepUp.error.timeout",
  PASSKEY_USER_MISMATCH: "security.stepUp.error.userMismatch",
  PASSKEY_UNKNOWN: "security.stepUp.error.unknownPasskey",
  FULL_SESSION_REQUIRED: "security.stepUp.error.fullSession",
} as const;

/**
 * The step-up's failure: LP's ceremony fails with a `PasskeyError` (desktop:
 * the bare code from Rust), the server with an `HttpError`.
 */
function stepUpErrorText(err: unknown): string {
  if (err instanceof PasskeyError) {
    const key = STEP_UP_ERRORS[err.code as keyof typeof STEP_UP_ERRORS];
    return key ? t(key) : formatPasskeyError(err);
  }
  return errorText(err, "security.stepUp.error.generic", STEP_UP_ERRORS);
}

export const StepUpDialog: Component<{ pending: PendingStepUp }> = (props) => {
  const label = () => t(stepUpLabel(props.pending.req), { target: props.pending.req.targetLabel });
  return (
    <DialogShell title={t("security.stepUp.title")} onClose={() => props.pending.cancel()} testId="step-up-dialog">
      <p class="text-lg font-medium text-text-primary">{label()}</p>
      <Show
        when={props.pending.mode === "passkey"}
        fallback={<p class="text-base text-text-primary">{t("security.stepUp.phoneOnly")}</p>}
      >
        <p class="text-base text-text-secondary">{t("security.stepUp.intro")}</p>
        <Show when={props.pending.running() && isTauri()}>
          <p class="text-base text-text-primary" role="status">
            {t("security.stepUp.continueInBrowser")}
          </p>
          <LinuxPasskeyHint class="text-base text-text-secondary" />
        </Show>
        <Show when={props.pending.error()}>
          {(err) => (
            <p role="alert" class="text-base text-text-primary">
              {stepUpErrorText(err())}
            </p>
          )}
        </Show>
        <button
          type="button"
          class="btn-primary w-full min-h-12 text-base"
          disabled={props.pending.running()}
          onClick={() => void props.pending.confirm()}
        >
          {t("security.stepUp.confirm")}
        </button>
      </Show>
      <button
        type="button"
        class="w-full min-h-12 rounded-lg border border-white/10 text-base text-text-primary hover:bg-white/5"
        onClick={() => props.pending.cancel()}
      >
        {t("common.cancel")}
      </button>
    </DialogShell>
  );
};

const StepUpHost: Component = () => (
  <Show when={pendingStepUp()} keyed>
    {(pending) => <StepUpDialog pending={pending} />}
  </Show>
);

export default StepUpHost;
