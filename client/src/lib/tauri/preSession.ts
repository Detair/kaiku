/**
 * Shared plumbing for calls made before a session exists (C27 pre-session
 * secrets): the browser sends them with its path-scoped HttpOnly cookies and
 * no bearer (`isPreSessionPath`); the desktop app sends them through Rust,
 * which keeps the request secret or registration ticket and adds its header
 * (`pre_session_request`). The webview never sees those secrets.
 */

import type { User } from "../types";
import {
  browserState,
  httpRequest,
  isTauri,
  scheduleTokenRefresh,
  setSessionRestoreBlocked,
  type AuthResponse,
  type AuthResult,
} from "./common";
import { nativeAuthError } from "./auth";

/** Which pre-session secret Rust attaches (`X-Kaiku-Request-Secret` / `X-Kaiku-Ticket`). */
export type PreSessionKind = "request" | "ticket";

/** Base URL without trailing slashes. */
export function baseOf(serverUrl: string): string {
  return serverUrl.trim().replace(/\/+$/, "");
}

/** Browser: remember the server the user signs in to (as `login` always did). */
export function useBrowserServer(serverUrl: string): void {
  const base = baseOf(serverUrl);
  browserState.serverUrl = base;
  try {
    localStorage.setItem("serverUrl", base);
  } catch {
    // Storage blocked: the in-memory URL still works for this tab.
  }
  setSessionRestoreBlocked(false);
}

/**
 * Browser: take over the access token of a session-creating answer (the
 * refresh token is an HttpOnly cookie), then load the profile.
 */
export async function adoptBrowserAuth(response: AuthResponse): Promise<AuthResult> {
  browserState.accessToken = response.access_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;
  scheduleTokenRefresh();
  const user = await httpRequest<User>("GET", "/auth/me");
  return {
    user,
    setup_required: response.setup_required,
    password_change_required: response.password_change_required ?? false,
  };
}

/** Native: run a Rust command and map its bare-code errors to `HttpError`s. */
export async function invokeNative<T>(cmd: (invoke: typeof import("@tauri-apps/api/core").invoke) => Promise<T>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await cmd(invoke);
  } catch (error) {
    throw nativeAuthError(error);
  }
}

/** A request authenticated by the request secret or the registration ticket. */
export async function preSessionRequest<T>(
  kind: PreSessionKind,
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  if (isTauri()) {
    return invokeNative((invoke) =>
      invoke<T>("pre_session_request", { kind, method, path, body: body ?? null }),
    );
  }
  return httpRequest<T>(method, path, body);
}
