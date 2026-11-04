/** Refusals of an account-request start (C27): one text for every cause of a 401. */

import { plural, t, type MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { codeIs } from "@/lib/errorCode";
import { HttpError } from "@/lib/tauri/common";
import { loginErrorText } from "@/lib/authErrors";

/** `bootstrap` words a 401 without the recovery code it does not have (Fable m5). */
export function requestStartError(err: unknown, purpose: "recovery" | "bootstrap" = "recovery"): string {
  if (err instanceof HttpError) {
    if (codeIs(err.code, "REQUEST_FACTOR_INVALID")) {
      const left = typeof err.details.recovery_codes_left === "number" ? err.details.recovery_codes_left : 0;
      return plural(left, { one: "auth.recover.factorInvalid.one", other: "auth.recover.factorInvalid.other" });
    }
    if (codeIs(err.code, "INVALID_CREDENTIALS") || err.status === 401) {
      return t(purpose === "bootstrap" ? "auth.secure.notAccepted" : "auth.recover.notAccepted");
    }
    if (codeIs(err.code, "TOO_MANY_ATTEMPTS")) return loginErrorText(err);
  }
  if (err instanceof Error && err.message === "POPUP_BLOCKED") return t("auth.login.popupBlocked");
  return errorText(err, "auth.recover.failed", {
    ACCOUNT_REQUEST_OPEN: "auth.recover.alreadyOpen",
    NO_INDEPENDENT_CANCELLER: "auth.recover.noCanceller",
    PASSKEY_BROWSER_FAILED: "passkeys.error.browserFailed",
    OIDC_PROVIDER_ERROR: "auth.sso.providerError",
    OIDC_EXCHANGE_FAILED: "auth.sso.exchangeFailed",
    OIDC_TIMEOUT: "auth.sso.timeout",
    SERVER_OUTDATED: "auth.login.serverOutdated",
  });
}

/** The steps of a running request (`/account-request`). */
const REQUEST_STEP_ERRORS = {
  NO_PRE_SESSION_SECRET: "auth.request.lost",
  // The browser's request secret is unknown or expired (the server's 404).
  NOT_FOUND: "auth.request.lost",
  ACCOUNT_REQUEST_COMPLETED: "auth.request.completed",
} as const;

/** A request step's refusal; 409 `ACCOUNT_REQUEST_NOT_READY {state}` says not yet, or expired (C27 amendment). */
export function requestStepError(err: unknown, fallback: MessageKey): string {
  if (err instanceof HttpError && codeIs(err.code, "ACCOUNT_REQUEST_NOT_READY")) {
    return t(err.details?.state === "expired" ? "auth.request.expiredNow" : "auth.request.notReady");
  }
  return errorText(err, fallback, REQUEST_STEP_ERRORS);
}

/** The registration ticket is gone: lost on this device, or unknown or expired on the server. */
export const BIND_ERRORS = {
  NO_PRE_SESSION_SECRET: "auth.bind.lost",
  REGISTRATION_TICKET_INVALID: "auth.bind.lost",
} as const;

export function bindErrorText(err: unknown, fallback: MessageKey): string {
  return errorText(err, fallback, BIND_ERRORS);
}
