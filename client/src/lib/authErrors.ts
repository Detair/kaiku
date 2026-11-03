import { plural, t, type MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { formatTimestamp } from "@/i18n/format";
import { codeIs } from "@/lib/errorCode";
import { HttpError } from "@/lib/tauri/common";
import { PasskeyError } from "@/lib/tauri/passkeys";

/** Server codes the sign-in form words specifically (used with `errorText`). */
export const LOGIN_ERRORS: Partial<Record<string, MessageKey>> = {
  // F3: one neutral answer for a wrong password, an unknown user and a
  // closed password path (also OIDC's neutral 401).
  INVALID_CREDENTIALS: "auth.login.neutralFailure",
  INVALID_MFA: "auth.login.mfaInvalid",
  ACCOUNT_BANNED: "auth.login.banned",
  TOO_MANY_ATTEMPTS: "auth.login.tooManyAttempts",
  CLIENT_OUTDATED: "auth.login.clientOutdated",
  LOGIN_ATTEMPT_ENDED: "auth.login.attemptEnded",
  LOGIN_ATTEMPT_UNKNOWN: "auth.login.attemptEnded",
  TOTP_UNAVAILABLE: "auth.login.totpUnavailable",
  // Desktop: a server without the v1.0 sign-in steps answered with tokens,
  // which Rust refuses (Fable review M1).
  SERVER_OUTDATED: "auth.login.serverOutdated",
};

/** Server codes the sign-up form words specifically (used with `errorText`). */
export const REGISTER_ERRORS: Partial<Record<string, MessageKey>> = {
  USER_EXISTS: "auth.register.usernameTaken",
  EMAIL_TAKEN: "auth.register.emailTaken",
  REGISTRATION_DISABLED: "auth.register.closed",
  SERVER_OUTDATED: "auth.login.serverOutdated",
};

/** A 409 without a code (a proxy, or a server without a JSON body). */
export const REGISTER_STATUS_ERRORS: Partial<Record<number, MessageKey>> = {
  409: "auth.register.alreadyExists",
};

/** Passkey failures (C29) the passkey UI words specifically. */
export const PASSKEY_ERRORS: Partial<Record<string, MessageKey>> = {
  PASSKEY_UNKNOWN: "passkeys.error.removed",
  PASSKEY_INVALID: "passkeys.error.invalid",
  PASSKEY_USER_MISMATCH: "passkeys.error.userMismatch",
  PASSKEY_CEREMONY_INVALID: "passkeys.error.expired",
  PASSKEY_NOT_SUPPORTED: "passkeys.error.notSupported",
  PASSKEY_NOT_DISCOVERABLE: "passkeys.error.notSupported",
  PASSKEY_FAILED: "passkeys.error.failed",
  PASSKEY_ALREADY_REGISTERED: "passkeys.error.alreadyRegistered",
  PASSKEY_LIMIT: "passkeys.error.limit",
  LAST_STRONG_AUTHENTICATOR: "passkeys.error.lastStrong",
  PASSKEY_BROWSER_FAILED: "passkeys.error.browserFailed",
  PASSKEY_TIMEOUT: "passkeys.error.timeout",
  CLIENT_OUTDATED: "passkeys.error.clientOutdated",
  PASSKEYS_UNAVAILABLE: "passkeys.error.unavailable",
  PASSKEY_HANDOFF_INVALID: "passkeys.error.handoff",
  STEP_UP_REQUIRED: "passkeys.error.stepUpRequired",
  FULL_SESSION_REQUIRED: "passkeys.error.fullSessionRequired",
  ACCOUNT_BANNED: "auth.login.banned",
};

/** The sentence for a passkey failure code. */
export function passkeyErrorKey(code: string): MessageKey {
  return PASSKEY_ERRORS[code] ?? "passkeys.error.failed";
}

/**
 * The sentence for a passkey failure, `PASSKEY_PENDING` with its ready time
 * (never "unknown", B.1). Silent codes (`ABORTED`, `PASSKEY_CANCELLED`) give "".
 */
export function formatPasskeyError(err: { code: string; readyAt?: string }): string {
  if (err.code === "ABORTED" || err.code === "PASSKEY_CANCELLED") return "";
  if (err.code === "PASSKEY_PENDING") {
    return t("passkeys.error.readyAt", { time: err.readyAt ? formatTimestamp(err.readyAt) : "" });
  }
  return t(passkeyErrorKey(err.code));
}

/**
 * Text for a refused sign-in step: the F3 neutral answer, and for
 * `TOO_MANY_ATTEMPTS` the wait from `Retry-After` in whole minutes.
 */
export function loginErrorText(err: unknown): string {
  const http =
    err instanceof HttpError ? err : err instanceof Error && err.cause instanceof HttpError ? err.cause : null;
  if (http && codeIs(http.code, "TOO_MANY_ATTEMPTS")) {
    if (http.retryAfter && http.retryAfter > 0) {
      const minutes = Math.max(1, Math.ceil(http.retryAfter / 60));
      return plural(minutes, {
        one: "auth.login.tooManyAttemptsMinutes.one",
        other: "auth.login.tooManyAttemptsMinutes.other",
      });
    }
    return t("auth.login.tooManyAttempts");
  }
  return errorText(err, "auth.login.failed", LOGIN_ERRORS);
}

/**
 * A failed passkey ceremony (LP's `PasskeyError`) in LP's words, silent ("")
 * for cancels and aborts; anything else through `errorText`.
 */
export function passkeyFailureText(
  err: unknown,
  fallback: MessageKey,
  byCode?: Partial<Record<string, MessageKey>>,
): string {
  if (err instanceof PasskeyError) {
    const key = byCode?.[err.code];
    return key ? t(key) : formatPasskeyError(err);
  }
  return errorText(err, fallback, byCode);
}
