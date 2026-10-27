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
