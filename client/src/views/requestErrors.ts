/** Refusals of an account-request start (C27): one text for every cause of a 401. */

import { plural, t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { codeIs } from "@/lib/errorCode";
import { HttpError } from "@/lib/tauri/common";
import { loginErrorText } from "@/lib/authErrors";

export function requestStartError(err: unknown): string {
  if (err instanceof HttpError) {
    if (codeIs(err.code, "REQUEST_FACTOR_INVALID")) {
      const left = typeof err.details.recovery_codes_left === "number" ? err.details.recovery_codes_left : 0;
      return plural(left, { one: "auth.recover.factorInvalid.one", other: "auth.recover.factorInvalid.other" });
    }
    if (codeIs(err.code, "INVALID_CREDENTIALS") || err.status === 401) return t("auth.recover.notAccepted");
    if (codeIs(err.code, "TOO_MANY_ATTEMPTS")) return loginErrorText(err);
  }
  if (err instanceof Error && err.message === "POPUP_BLOCKED") return t("auth.login.popupBlocked");
  return errorText(err, "auth.recover.failed", {
    ACCOUNT_REQUEST_OPEN: "auth.recover.alreadyOpen",
    NO_INDEPENDENT_CANCELLER: "auth.recover.noCanceller",
    OIDC_UNAVAILABLE: "auth.recover.providerUnavailable",
  });
}
