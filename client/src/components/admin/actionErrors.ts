/**
 * Turns a failed admin or server-settings call into a sentence that says
 * what happened and what to do (ADMIN-1, ADMIN-6). The admin mode running
 * out is the most common cause and gets its own sentence.
 */
import { errorText } from "@/i18n/errors";
import type { MessageKey } from "@/i18n";

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

/** Sentence for a failed action; `fallback` when the cause is unknown. */
export function actionErrorText(
  err: unknown,
  fallback: MessageKey = "guildSettings.error.tryAgain",
): string {
  return errorText(err, fallback, BY_CODE, BY_STATUS);
}
