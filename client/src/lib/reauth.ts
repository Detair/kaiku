/**
 * Re-authentication methods (contract C24).
 *
 * Which methods the dialog offers is decided here, behind a small strategy
 * interface, so the dialog never hardcodes "password".
 */

import { HttpError } from "@/lib/tauri";
import { t } from "@/i18n";
import type { MessageKey } from "@/i18n";

export type ReauthMethod =
  | { kind: "password" }
  | { kind: "oidc"; providerId: string; label: string };

/** What the client knows about how the signed-in user can prove who they are. */
export interface ReauthContext {
  hasLocalPassword: boolean;
  /** Linked OIDC providers the user could confirm with. */
  oidcProviders: { id: string; label: string }[];
  /** True once the server defines the OIDC re-auth flow (see TODO below). */
  oidcReauthSupported: boolean;
}

export interface ReauthStrategy {
  availableMethods(): ReauthMethod[];
  /** Fresh provider login (prompt=login, max_age=0); resolves once elevated. */
  reauthWithOidc(providerId: string): Promise<void>;
}

/** Pure method selection from what is known about the account. */
export function methodsFor(ctx: ReauthContext): ReauthMethod[] {
  const methods: ReauthMethod[] = [];
  if (ctx.hasLocalPassword) methods.push({ kind: "password" });
  if (ctx.oidcReauthSupported) {
    for (const p of ctx.oidcProviders) {
      methods.push({ kind: "oidc", providerId: p.id, label: p.label });
    }
  }
  return methods;
}

// TODO(C24-OIDC): the server has not yet defined the re-auth method list or the
// OIDC re-auth flow. Until it does, the client only knows the password path and
// the OIDC path stays hidden. To switch it on: (1) fill `oidcProviders` and set
// `oidcReauthSupported` from the server's method list, (2) implement
// `reauthWithOidc` against the server's re-auth endpoint (fresh login with
// prompt=login and max_age=0, then the session is elevated).
function currentContext(): ReauthContext {
  return { hasLocalPassword: true, oidcProviders: [], oidcReauthSupported: false };
}

export const defaultReauthStrategy: ReauthStrategy = {
  availableMethods: () => methodsFor(currentContext()),
  reauthWithOidc: async () => {
    throw new Error("OIDC re-authentication is not available yet");
  },
};

const REAUTH_ERRORS: Record<string, MessageKey> = {
  INVALID_CREDENTIALS: "admin.reauth.error.invalidCredentials",
  MFA_CODE_REQUIRED: "admin.reauth.error.mfaRequired",
  REAUTH_UNAVAILABLE: "admin.reauth.error.unavailable",
  VALIDATION_ERROR: "admin.reauth.error.validation",
};

/** User-facing text for a failed re-authentication call. */
export function reauthErrorText(err: unknown): string {
  if (err instanceof HttpError) {
    const key = err.code ? REAUTH_ERRORS[err.code] : undefined;
    if (key) return t(key);
    if (err.status === 429) return t("admin.reauth.error.rateLimited");
  }
  return t("admin.reauth.error.generic");
}
