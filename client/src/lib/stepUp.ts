/**
 * Step-up for sensitive actions (F7, C26–C29 common rules).
 *
 * `withStepUp(req, run)`: a chat-only session gets the full-session dialog
 * and no ceremony; otherwise the step-up dialog names the action and target,
 * runs the passkey ceremony (desktop: in the system browser) and then
 * `run()`. The server keeps the grant (bound to the session, single use), so
 * `run()` just performs the action. A 403 `STEP_UP_REQUIRED` from `run()`
 * gets exactly one new step-up and one retry. Closing the dialog aborts the
 * ceremony and rejects with `StepUpCancelled`. Desktop and web step up only
 * by passkey; without one the dialog says to use the Kaiku app on the phone.
 */

import { createSignal } from "solid-js";
import type { MessageKey } from "@/i18n";
import { codeIs } from "@/lib/errorCode";
import { HttpError } from "@/lib/tauri/common";
import { getStepUpMethods } from "@/lib/tauri/security";
import type { StepUpAction } from "@/lib/types";
import { passkeyCeremonies } from "@/lib/passkeySeam";
import { isProbation } from "@/stores/auth";
import { showFullSessionRequired } from "@/components/auth/FullSessionRequiredDialog";

export interface StepUpRequest {
  action: StepUpAction;
  /** Canonical target: lowercase UUID, `others` or `""`. */
  target: string;
  /** What the dialog shows for the target ("Pixel 8"). */
  targetLabel: string;
}

/** The user closed the step-up (or the session cannot step up). */
export class StepUpCancelled extends Error {
  constructor(message = "STEP_UP_CANCELLED") {
    super(message);
    this.name = "StepUpCancelled";
  }
}

/** The action table, one literal key per action (i18n:check sees each). */
export const STEP_UP_LABELS: Record<StepUpAction, MessageKey> = {
  "passkey.add": "security.stepUp.action.passkeyAdd",
  "passkey.remove": "security.stepUp.action.passkeyRemove",
  "key_device.remove": "security.stepUp.action.keyDeviceRemove",
  "enrollment.issue": "security.stepUp.action.enrollmentIssue",
  "password.set": "security.stepUp.action.passwordSet",
  "password.remove": "security.stepUp.action.passwordRemove",
  "totp.set": "security.stepUp.action.totpSet",
  "totp.remove": "security.stepUp.action.totpRemove",
  "recovery_codes.create": "security.stepUp.action.recoveryCodesCreate",
  "identity.link": "security.stepUp.action.identityLink",
  "identity.unlink": "security.stepUp.action.identityUnlink",
  "sessions.end": "security.stepUp.action.sessionsEnd",
  "recovery_contact.set": "security.stepUp.action.recoveryContactSet",
  "request.cancel": "security.stepUp.action.requestCancel",
  "event.repudiate": "security.stepUp.action.eventRepudiate",
  "account.delete": "security.stepUp.action.accountDelete",
  "admin.elevate": "security.stepUp.action.adminElevate",
  "admin.enrollment.issue": "security.stepUp.action.adminEnrollmentIssue",
  "admin.authenticator.remove": "security.stepUp.action.adminAuthenticatorRemove",
  "admin.may_approve": "security.stepUp.action.adminMayApprove",
  "admin.factor.set_state": "security.stepUp.action.adminFactorSetState",
  "admin.recovery_contact.set": "security.stepUp.action.adminRecoveryContactSet",
};

/** The label key for a request ("End all other sessions" for `others`). */
export function stepUpLabel(req: StepUpRequest): MessageKey {
  if (req.action === "sessions.end" && req.target === "others") return "security.stepUp.action.sessionsEndOthers";
  return STEP_UP_LABELS[req.action];
}

/** What the open step-up dialog shows. */
export interface PendingStepUp {
  req: StepUpRequest;
  /** `passkey`: a ceremony can run; `phone`: only the phone can confirm. */
  mode: "passkey" | "phone";
  /** Run the ceremony (the dialog's button). */
  confirm: () => Promise<void>;
  /** Close: aborts a running ceremony. */
  cancel: () => void;
  running: () => boolean;
  error: () => unknown;
}

const [pending, setPending] = createSignal<PendingStepUp | null>(null);

/** The step-up the dialog host shows, if any. */
export const pendingStepUp = pending;

/** Open the dialog and wait for one grant. Rejects `StepUpCancelled` on close. */
function requestGrant(req: StepUpRequest, passkeyOk: boolean): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const abort = new AbortController();
    const [running, setRunning] = createSignal(false);
    const [error, setError] = createSignal<unknown>(null);
    const ceremonies = passkeyCeremonies();
    const entry: PendingStepUp = {
      req,
      mode: passkeyOk && ceremonies ? "passkey" : "phone",
      running,
      error,
      confirm: async () => {
        if (!ceremonies || running()) return;
        setRunning(true);
        setError(null);
        try {
          await ceremonies.stepUp(req.action, req.target, abort.signal);
          if (abort.signal.aborted) return;
          setPending(null);
          resolve();
        } catch (err) {
          if (!abort.signal.aborted) setError(err);
        } finally {
          setRunning(false);
        }
      },
      cancel: () => {
        abort.abort();
        setPending(null);
        reject(new StepUpCancelled());
      },
    };
    setPending(entry);
  });
}

function isStepUpRequired(err: unknown): boolean {
  return err instanceof HttpError && err.status === 403 && codeIs(err.code, "STEP_UP_REQUIRED");
}

export async function withStepUp<T>(req: StepUpRequest, run: () => Promise<T>): Promise<T> {
  if (isProbation()) {
    showFullSessionRequired();
    throw new StepUpCancelled("FULL_SESSION_REQUIRED");
  }
  let passkeyOk = false;
  try {
    passkeyOk = (await getStepUpMethods()).passkey;
  } catch (err) {
    console.warn("[StepUp] Loading the methods failed:", err instanceof Error ? err.message : err);
  }
  await requestGrant(req, passkeyOk);
  try {
    return await run();
  } catch (err) {
    if (!isStepUpRequired(err)) throw err;
    // The grant expired or was spent elsewhere: one new step-up, one retry.
    await requestGrant(req, passkeyOk);
    return run();
  }
}
