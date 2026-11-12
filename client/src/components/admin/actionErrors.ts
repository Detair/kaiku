/**
 * Turns a failed admin or server-settings call into a sentence that says
 * what happened and what to do (ADMIN-1, ADMIN-6). The admin mode running
 * out is the most common cause and gets its own sentence.
 */
import { errorText } from "@/i18n/errors";
import type { MessageKey } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { markElevationEnded } from "@/stores/admin";

const BY_CODE: Partial<Record<string, MessageKey>> = {
  ELEVATION_REQUIRED: "admin.mode.expiredRetry",
  ADMIN_NOT_ELEVATED: "admin.mode.expiredRetry",
  FORBIDDEN: "guildSettings.error.notAllowed",
  PERMISSION_DENIED: "guildSettings.error.notAllowed",
  NOT_FOUND: "guildSettings.error.gone",
};

const BY_STATUS: Partial<Record<number, MessageKey>> = {
  403: "guildSettings.error.notAllowed",
  404: "guildSettings.error.gone",
};

/**
 * The server says admin mode is over (Redis TTL, clock skew, another tab):
 * switch the display off too, so the header never says "on" after this (K3).
 */
export function noteElevationEnded(err: unknown): void {
  if (err instanceof HttpError && (codeIs(err.code, "ELEVATION_REQUIRED") || codeIs(err.code, "ADMIN_NOT_ELEVATED"))) {
    markElevationEnded();
  }
}

const ADMIN_BY_CODE: Partial<Record<string, MessageKey>> = {
  ...BY_CODE,
  FORBIDDEN: "admin.error.notAllowed",
  PERMISSION_DENIED: "admin.error.notAllowed",
  NOT_ADMIN: "admin.error.notAllowed",
};
const ADMIN_BY_STATUS: Partial<Record<number, MessageKey>> = { ...BY_STATUS, 403: "admin.error.notAllowed" };

/**
 * Sentence for a failed action; `fallback` when the cause is unknown.
 * Fallbacks from the admin area (`admin.*`) get admin sentences for a 403,
 * not "ask the owner of the server" (K6).
 */
export function actionErrorText(
  err: unknown,
  fallback: MessageKey = "guildSettings.error.tryAgain",
): string {
  noteElevationEnded(err);
  const admin = fallback.startsWith("admin.");
  return errorText(err, fallback, admin ? ADMIN_BY_CODE : BY_CODE, admin ? ADMIN_BY_STATUS : BY_STATUS);
}
