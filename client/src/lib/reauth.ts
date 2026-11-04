/**
 * Wording for a failed confirmation of a sensitive action (F7, C27).
 *
 * The C24 re-authentication (password, authenticator code, a fresh provider
 * sign-in) is gone: sensitive actions take a step-up grant from a passkey or
 * the Kaiku app on the phone (`lib/stepUp.ts`). What stays is one text
 * mapping for the codes those actions answer with.
 */

import { HttpError } from "@/lib/tauri/common";
import { normalizeCode } from "@/lib/errorCode";
import type { MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { formatPasskeyError } from "@/lib/authErrors";
import { PasskeyError } from "@/lib/tauri/passkeys";

const REAUTH_ERRORS: Record<string, MessageKey> = {
  // `admin.*` step-ups and `/api/admin/*` need a system admin (LP round 2).
  NOT_ADMIN: "security.stepUp.error.notAdmin",
  FULL_SESSION_REQUIRED: "passkeys.error.fullSessionRequired",
  STEP_UP_REQUIRED: "passkeys.error.stepUpRequired",
  SERVICE_UNAVAILABLE: "admin.reauth.error.serviceUnavailable",
};

const REAUTH_STATUS_ERRORS: Partial<Record<number, MessageKey>> = {
  429: "admin.reauth.error.rateLimited",
  403: "admin.reauth.error.forbidden",
  503: "admin.reauth.error.serviceUnavailable",
};

/**
 * The `HttpError` behind a failure, unwrapped through `cause` the same way
 * `errorText()` does, with its code upper-cased (servers spell some codes in
 * lowercase). `undefined` when no HTTP error is involved.
 */
function normalisedHttpError(err: unknown): HttpError | undefined {
  let current: unknown = err;
  for (let depth = 0; depth < 8 && current !== undefined; depth++) {
    if (current instanceof HttpError) {
      const code = normalizeCode(current.code);
      return code !== null && code !== current.code
        ? new HttpError(current.status, current.message, code, current.details, current.retryAfter)
        : current;
    }
    current = current instanceof Error ? current.cause : undefined;
  }
  return undefined;
}

/**
 * User-facing text for a failed confirmation or the action behind it (never
 * server text): LP's passkey wording for a ceremony failure, else the codes
 * above on top of `errorText()`.
 */
export function reauthErrorText(err: unknown): string {
  if (err instanceof PasskeyError) return formatPasskeyError(err);
  return errorText(normalisedHttpError(err) ?? err, "admin.reauth.error.generic", REAUTH_ERRORS, REAUTH_STATUS_ERRORS);
}
