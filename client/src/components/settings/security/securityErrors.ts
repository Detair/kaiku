/** Server codes the security settings word specifically (C26–C28). */

import type { MessageKey } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
import { errorText } from "@/i18n/errors";
import { StepUpCancelled } from "@/lib/stepUp";
import { codeIs } from "@/lib/errorCode";
import { totalKey } from "@/lib/totalKey";
import { loginErrorText } from "@/lib/authErrors";

/**
 * The one map for security actions (Fable review m5): settings, notices,
 * admin panels and linked accounts.
 */
export const SECURITY_ERRORS: Partial<Record<string, MessageKey>> = {
  LAST_STRONG_AUTHENTICATOR: "security.error.lastStrong",
  RECOVERY_CONTACT_INVALID: "security.error.contactInvalid",
  FULL_SESSION_REQUIRED: "security.stepUp.error.fullSession",
  // The grant expired or was spent elsewhere: confirm again.
  STEP_UP_REQUIRED: "passkeys.error.stepUpRequired",
  NOT_ADMIN: "security.stepUp.error.notAdmin",
  ADMIN_TARGET_IS_ADMIN: "security.error.adminTarget",
  ENROLLMENT_LIMIT: "security.setupPhone.limit",
  // C26/C27 amendment 2026-11-03.
  ENROLLMENT_DEVICE_IN_USE: "security.error.deviceInUse",
  ENROLLMENT_TARGET_SELF: "security.error.targetSelf",
  // A database or Valkey outage; a retry can succeed.
  SERVICE_UNAVAILABLE: "security.error.serviceUnavailable",
};

const PASSWORD_REASONS: Record<string, MessageKey> = {
  length: "security.password.rejected.length",
  breached: "security.password.rejected.breached",
  context: "security.password.rejected.context",
};

/**
 * Text for a failed security action, or null when the user just cancelled.
 * `extra` adds the codes of one place (notices). 429 says how long to wait.
 */
export function securityErrorText(
  err: unknown,
  fallback: MessageKey,
  extra?: Partial<Record<string, MessageKey>>,
): string | null {
  if (err instanceof StepUpCancelled) return null;
  if (err instanceof HttpError && (codeIs(err.code, "TOO_MANY_ATTEMPTS") || err.status === 429)) {
    return loginErrorText(err);
  }
  if (err instanceof HttpError && codeIs(err.code, "PASSWORD_REJECTED")) {
    // `details` may be missing from a proxy's or an older server's body (Flash r3 p1 #5).
    return errorText(err, totalKey(PASSWORD_REASONS, err.details?.reason, "security.password.rejected.generic"));
  }
  return errorText(err, fallback, extra ? { ...SECURITY_ERRORS, ...extra } : SECURITY_ERRORS);
}
