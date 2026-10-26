/**
 * Re-authentication methods and flows (contract C24).
 *
 * The server says which methods this session can use; the dialog offers only
 * those. Both the password path and the provider path end in the same shared
 * error mapping (`reauthErrorText`).
 */

import {
  getReauthMethods,
  startOidcReauth,
  getServerUrl,
  isTauri,
  HttpError,
} from "@/lib/tauri";
import type { ReauthMethodsResponse } from "@/lib/types";
import { t } from "@/i18n";
import type { MessageKey } from "@/i18n";

export type ReauthMethod =
  | { kind: "password" }
  | { kind: "oidc"; providerId: string; label: string };

export interface ReauthInfo {
  methods: ReauthMethod[];
  /** An authenticator code is needed on every path. */
  mfaRequired: boolean;
}

export interface ReauthStrategy {
  loadInfo(): Promise<ReauthInfo>;
  /** Fresh provider login; resolves once the provider flow reported success. */
  reauthWithOidc(
    providerId: string,
    opts: { reason?: string | null; mfaCode?: string | null; signal: AbortSignal },
  ): Promise<void>;
}

/** Map the server's method list to what the dialog shows. */
export function infoFromServer(r: ReauthMethodsResponse): ReauthInfo {
  const methods: ReauthMethod[] = [];
  if (r.password) methods.push({ kind: "password" });
  for (const p of r.oidc) {
    methods.push({ kind: "oidc", providerId: p.provider_id, label: p.display_name });
  }
  return { methods, mfaRequired: r.mfa_required };
}

/** Error codes raised by the client side of the provider flow. */
const CANCELLED = "REAUTH_CANCELLED";
const TIMEOUT = "REAUTH_TIMEOUT";
const POPUP_BLOCKED = "REAUTH_POPUP_BLOCKED";

/** Longest the browser flow waits for the provider's answer. */
const BROWSER_TIMEOUT_MS = 120_000;

function abortError(): Error {
  const e = new Error("aborted");
  e.name = "AbortError";
  return e;
}

interface CallbackMessage {
  type: "oidc-reauth-callback";
  success: boolean;
  error_code: string | null;
  expires_at: string | null;
}

function isCallbackMessage(d: unknown): d is CallbackMessage {
  if (typeof d !== "object" || d === null) return false;
  const m = d as Record<string, unknown>;
  const strOrNull = (v: unknown) => v === null || v === undefined || typeof v === "string";
  return (
    m.type === "oidc-reauth-callback" &&
    typeof m.success === "boolean" &&
    strOrNull(m.error_code) &&
    strOrNull(m.expires_at)
  );
}

/**
 * Browser flow: send the (already opened) popup to `url` and wait for the
 * server's `oidc-reauth-callback` message. Only a well-formed message from the
 * server origin and from this popup counts. Aborting, a closed popup and the
 * time ceiling all stop listening and close the popup. Success is only a hint;
 * the caller confirms elevation with the server.
 */
export function waitForBrowserReauth(
  popup: Window,
  url: string,
  signal: AbortSignal,
  timeoutMs: number = BROWSER_TIMEOUT_MS,
): Promise<void> {
  const expectedOrigin = new URL(getServerUrl()).origin;
  return new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      popup.close();
      reject(abortError());
      return;
    }
    const fail = (code: string) => new HttpError(0, code, code);
    const timers: {
      poll?: ReturnType<typeof setInterval>;
      ceiling?: ReturnType<typeof setTimeout>;
    } = {};
    const finish = (fn: () => void, closePopup: boolean) => {
      window.removeEventListener("message", onMessage);
      signal.removeEventListener("abort", onAbort);
      clearInterval(timers.poll);
      clearTimeout(timers.ceiling);
      if (closePopup) popup.close();
      fn();
    };
    const onAbort = () => finish(() => reject(abortError()), true);
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== expectedOrigin || event.source !== popup) return;
      if (!isCallbackMessage(event.data)) return;
      const d = event.data;
      if (d.success) finish(resolve, false);
      else finish(() => reject(fail(d.error_code ?? "REAUTH_FAILED")), false);
    };
    window.addEventListener("message", onMessage);
    signal.addEventListener("abort", onAbort);
    timers.poll = setInterval(() => {
      if (popup.closed) finish(() => reject(fail(CANCELLED)), false);
    }, 500);
    timers.ceiling = setTimeout(() => finish(() => reject(fail(TIMEOUT)), true), timeoutMs);
    popup.location.href = url;
  });
}

/** Resolve with `p`, or reject as aborted as soon as `signal` fires. */
function untilAborted<T>(p: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(abortError());
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(abortError());
    signal.addEventListener("abort", onAbort, { once: true });
    p.then(resolve, reject).finally(() => signal.removeEventListener("abort", onAbort));
  });
}

export const defaultReauthStrategy: ReauthStrategy = {
  loadInfo: async () => infoFromServer(await getReauthMethods()),
  reauthWithOidc: async (providerId, opts) => {
    // The popup must open inside the user's click, before any request, or the
    // browser may block it after the server already used the code.
    const popup = isTauri() ? null : window.open("about:blank", `oidc-reauth-${Date.now()}`, "width=600,height=700");
    if (!isTauri() && !popup) throw new HttpError(0, POPUP_BLOCKED, POPUP_BLOCKED);
    try {
      const started = await untilAborted(startOidcReauth(providerId, opts), opts.signal);
      if (started.mode === "browser" && popup) {
        await waitForBrowserReauth(popup, started.url, opts.signal);
      }
    } catch (err) {
      popup?.close();
      throw err;
    }
  },
};

const REAUTH_ERRORS: Record<string, MessageKey> = {
  INVALID_CREDENTIALS: "admin.reauth.error.invalidCredentials",
  MFA_CODE_REQUIRED: "admin.reauth.error.mfaRequired",
  REAUTH_UNAVAILABLE: "admin.reauth.error.unavailable",
  REAUTH_OIDC_REQUIRED: "admin.reauth.error.oidcRequired",
  REAUTH_PROVIDER_UNAVAILABLE: "admin.reauth.error.providerUnavailable",
  VALIDATION_ERROR: "admin.reauth.error.validation",
  REAUTH_STALE: "admin.reauth.error.stale",
  REAUTH_AUTH_TIME_MISSING: "admin.reauth.error.generic",
  REAUTH_IDENTITY_MISMATCH: "admin.reauth.error.notAllowed",
  REAUTH_IDENTITY_NOT_ELIGIBLE: "admin.reauth.error.notAllowed",
  REAUTH_NOT_ALLOWED: "admin.reauth.error.notAllowed",
  REAUTH_SESSION_INVALID: "admin.reauth.error.sessionInvalid",
  OIDC_EXCHANGE_FAILED: "admin.reauth.error.generic",
  OIDC_STATE_MISMATCH: "admin.reauth.error.generic",
  REAUTH_FAILED: "admin.reauth.error.generic",
  REAUTH_TIMEOUT: "admin.reauth.error.timeout",
  REAUTH_POPUP_BLOCKED: "admin.reauth.error.popupBlocked",
  REAUTH_NOT_CONFIRMED: "admin.reauth.notConfirmed",
  REAUTH_CANCELLED: "admin.reauth.error.cancelled",
  SERVICE_UNAVAILABLE: "admin.reauth.error.serviceUnavailable",
};

/** The server error code carried by a failed call, if any. */
export function reauthErrorCode(err: unknown): string | undefined {
  return err instanceof HttpError ? err.code : undefined;
}

/** User-facing text for a failed re-authentication call (never server text). */
export function reauthErrorText(err: unknown): string {
  if (err instanceof HttpError) {
    const key = err.code ? REAUTH_ERRORS[err.code] : undefined;
    if (key) return t(key);
    if (err.status === 429) return t("admin.reauth.error.rateLimited");
    if (err.status === 403) return t("admin.reauth.error.forbidden");
    if (err.status === 503) return t("admin.reauth.error.serviceUnavailable");
  }
  return t("admin.reauth.error.generic");
}
