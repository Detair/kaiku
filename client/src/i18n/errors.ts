import { HttpError } from "@/lib/tauri/common";
import { normalizeCode } from "@/lib/errorCode";
import { t, type MessageKey } from "./index";

type ByCode = Partial<Record<string, MessageKey>>;
type ByStatus = Partial<Record<number, MessageKey>>;

/** Own-property check (the TS lib target predates `Object.hasOwn`). */
function hasOwn(obj: object, key: string | number): boolean {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

/** `byCode[code]`, matching the map's keys case-insensitively (`code` is normalised). */
function lookupCode(byCode: ByCode, code: string): MessageKey | undefined {
  if (hasOwn(byCode, code)) return byCode[code];
  for (const key of Object.keys(byCode)) {
    if (normalizeCode(key) === code) return byCode[key];
  }
  return undefined;
}

/** Error carrying a `cause` (the auth store wraps failures that way). */
function causeOf(err: unknown): unknown {
  return err instanceof Error ? err.cause : undefined;
}

/**
 * Reasons a message send or edit can fail (codes from server/src/chat/error.rs).
 * Pass as `byCode`/`byStatus` so a muted or blocked user is not told to retry.
 */
export const CHAT_SEND_ERRORS: { byCode: ByCode; byStatus: ByStatus } = {
  byCode: {
    FORBIDDEN: "chat.send.forbidden",
    PERMISSION_DENIED: "chat.send.forbidden",
    NOT_MEMBER: "chat.send.forbidden",
    LIMIT_EXCEEDED: "chat.send.limitExceeded",
    BLOCKED: "chat.send.blocked",
    CONTENT_FILTERED: "chat.send.contentFiltered",
    FILE_TOO_LARGE: "chat.send.fileTooLarge",
    CHANNEL_NOT_FOUND: "chat.send.channelGone",
  },
  byStatus: { 413: "chat.send.fileTooLarge" },
};

/** Reasons starting a DM call can fail (server answers 403 BLOCKED). */
export const CALL_START_ERRORS: { byCode: ByCode } = {
  byCode: {
    BLOCKED: "voice.call.blocked",
    CALL_CONFLICT: "voice.call.error.conflict",
    CALL_ALREADY_EXISTS: "voice.call.error.alreadyExists",
    call_already_exists: "voice.call.error.alreadyExists",
  },
};

/**
 * Codes the SSO flows (desktop sign-in, identity linking) report; see
 * `nativeSsoError` for the native side.
 */
export const SSO_ERRORS: { byCode: ByCode } = {
  byCode: {
    OIDC_CLIENT_OUTDATED: "auth.sso.clientOutdated",
    OIDC_HANDOFF_INVALID: "auth.sso.handoffInvalid",
    OIDC_PROVIDER_ERROR: "auth.sso.providerError",
    OIDC_EXCHANGE_FAILED: "auth.sso.exchangeFailed",
    IDENTITY_ALREADY_LINKED: "auth.sso.alreadyLinked",
  },
};

/**
 * Turn any thrown value into a sentence for the user (contract C16).
 *
 * Lookup order: `byCode[code]`, `byStatus[status]`, common codes (rate limit,
 * server error), a failed `fetch` (network), a `VALIDATION_ERROR` message
 * verbatim, then `fallback`. The raw server message, a status line like
 * `HTTP 502: Bad Gateway` and unknown codes are never shown. Plain strings
 * (native `session_request` errors from Rust) have no code and get the
 * fallback.
 */
export function errorText(
  err: unknown,
  fallback: MessageKey,
  byCode?: ByCode,
  byStatus?: ByStatus,
): string {
  const http = err instanceof HttpError ? err : undefined;
  const cause = causeOf(err);
  if (!http && cause !== undefined) return errorText(cause, fallback, byCode, byStatus);

  const code = normalizeCode(http?.code);
  const status = http?.status;

  const byCodeKey = code && byCode ? lookupCode(byCode, code) : undefined;
  if (byCodeKey) return t(byCodeKey);
  const byStatusKey =
    status !== undefined && byStatus && hasOwn(byStatus, status) ? byStatus[status] : undefined;
  if (byStatusKey) return t(byStatusKey);

  if (code === "FEATURE_DISABLED") return t("errors.featureDisabled");
  if (code === "RATE_LIMITED" || status === 429) return t("errors.rateLimited");
  if (code === "INTERNAL_ERROR" || (status !== undefined && status >= 500)) {
    return t("errors.server");
  }
  if (err instanceof TypeError) return t("errors.network");
  // An empty server message makes HttpError.message fall back to the code.
  if (code === "VALIDATION_ERROR" && http?.message && normalizeCode(http.message) !== code) {
    return http.message;
  }
  return t(fallback);
}
