import { t } from "@/i18n";
import type { MessageKey } from "@/i18n";
import { normalizeCode } from "@/lib/errorCode";

const BY_CODE: Partial<Record<string, MessageKey>> = {
  NO_PERMISSION: "voice.screenShare.error.noPermission",
  LIMIT_REACHED: "voice.screenShare.error.limitReached",
  NOT_IN_CHANNEL: "voice.screenShare.error.notInChannel",
};

/**
 * Text for the `error` code of a screen-share permission check (C16: the
 * server's `message` is never shown). Accepts either casing of the code.
 */
export function screenShareCheckErrorText(code: unknown): string {
  const normalized = normalizeCode(code);
  const key = normalized !== null && Object.prototype.hasOwnProperty.call(BY_CODE, normalized)
    ? BY_CODE[normalized]
    : undefined;
  return t(key ?? "voice.screenShare.error.generic");
}
