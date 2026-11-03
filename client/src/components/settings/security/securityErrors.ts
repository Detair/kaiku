/** Server codes the security settings word specifically (C26–C28). */

import type { MessageKey } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
import { errorText } from "@/i18n/errors";
import { StepUpCancelled } from "@/lib/stepUp";

export const SECURITY_ERRORS: Partial<Record<string, MessageKey>> = {
  LAST_STRONG_AUTHENTICATOR: "security.error.lastStrong",
  RECOVERY_CONTACT_INVALID: "security.error.contactInvalid",
  FULL_SESSION_REQUIRED: "security.stepUp.error.fullSession",
  STEP_UP_REQUIRED: "security.stepUp.error.generic",
  ADMIN_TARGET_IS_ADMIN: "security.error.adminTarget",
};

const PASSWORD_REASONS: Record<string, MessageKey> = {
  length: "security.password.rejected.length",
  breached: "security.password.rejected.breached",
  context: "security.password.rejected.context",
};

/** Text for a failed security action, or null when the user just cancelled. */
export function securityErrorText(err: unknown, fallback: MessageKey): string | null {
  if (err instanceof StepUpCancelled) return null;
  if (err instanceof HttpError && err.code === "PASSWORD_REJECTED") {
    const reason = typeof err.details.reason === "string" ? err.details.reason : "";
    return errorText(err, PASSWORD_REASONS[reason] ?? "security.password.rejected.generic");
  }
  return errorText(err, fallback, SECURITY_ERRORS);
}
