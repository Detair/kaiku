/**
 * Re-authentication methods and flows (contract C24).
 *
 * The server says which methods this session can use; the dialog offers only
 * those. Both the password path and the provider path end in the same shared
 * error mapping (`reauthErrorText`).
 */

import { getReauthMethods, startOidcReauth, getServerUrl, HttpError } from "@/lib/tauri";
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
    opts: { reason?: string | null; mfaCode?: string | null },
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

/** Error raised when the user closes the provider window without finishing. */
const CANCELLED = "REAUTH_CANCELLED";

/**
 * Browser flow: open `url` in a popup and wait for the server's
 * `oidc-reauth-callback` message. Messages from other origins are ignored.
 * Success is only a hint; the caller confirms elevation with the server.
 */
export function waitForBrowserReauth(url: string): Promise<void> {
  const expectedOrigin = new URL(getServerUrl()).origin;
  return new Promise<void>((resolve, reject) => {
    const done = (fn: () => void) => {
      window.removeEventListener("message", onMessage);
      clearInterval(timer);
      fn();
    };
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== expectedOrigin) return;
      if (event.data?.type !== "oidc-reauth-callback") return;
      if (event.data.success) {
        done(resolve);
      } else {
        const code = String(event.data.error_code ?? "REAUTH_FAILED");
        done(() => reject(new HttpError(0, code, code)));
      }
    };
    window.addEventListener("message", onMessage);
    const popup = window.open(url, "oidc-reauth", "width=600,height=700");
    const timer = setInterval(() => {
      if (popup?.closed) done(() => reject(new HttpError(0, CANCELLED, CANCELLED)));
    }, 500);
    if (!popup) done(() => reject(new HttpError(0, CANCELLED, CANCELLED)));
  });
}

export const defaultReauthStrategy: ReauthStrategy = {
  loadInfo: async () => infoFromServer(await getReauthMethods()),
  reauthWithOidc: async (providerId, opts) => {
    const started = await startOidcReauth(providerId, opts);
    if (started.mode === "browser") await waitForBrowserReauth(started.url);
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
