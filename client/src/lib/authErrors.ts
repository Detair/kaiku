import type { MessageKey } from "@/i18n";

/** Server codes the sign-in form words specifically (used with `errorText`). */
export const LOGIN_ERRORS: Partial<Record<string, MessageKey>> = {
  INVALID_CREDENTIALS: "auth.login.invalidCredentials",
  INVALID_MFA: "auth.login.mfaInvalid",
  ACCOUNT_BANNED: "auth.login.banned",
};

/** Server codes the sign-up form words specifically (used with `errorText`). */
export const REGISTER_ERRORS: Partial<Record<string, MessageKey>> = {
  USER_EXISTS: "auth.register.usernameTaken",
  EMAIL_TAKEN: "auth.register.emailTaken",
  REGISTRATION_DISABLED: "auth.register.closed",
};

/** A bare 409 (native sign-up cannot tell username from email). */
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
