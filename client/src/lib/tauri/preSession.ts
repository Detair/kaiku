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
  clearBrowserTokens,
  HttpError,
  httpRequest,
  isTauri,
  scheduleTokenRefresh,
  setBrowserToken,
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

/** Whether the caller still wants a session that is being made. */
export type StillWanted = () => boolean;

/** The sign-in was abandoned while its session was made (C27 shape). */
export function abandonedSignIn(): HttpError {
  return new HttpError(410, "LOGIN_ATTEMPT_ENDED", "LOGIN_ATTEMPT_ENDED", { state: "cancelled" });
}

/** `GET /auth/me` with an explicit server and bearer (not the global ones). */
async function fetchMeWith(base: string, accessToken: string): Promise<User> {
  const response = await fetch(`${base}/auth/me`, {
    method: "GET",
    headers: { Authorization: `Bearer ${accessToken}` },
    credentials: "omit",
  });
  if (!response.ok) {
    let code: string | undefined;
    try {
      const body = (await response.json()) as { error?: unknown };
      if (typeof body?.error === "string") code = body.error;
    } catch {
      code = undefined;
    }
    throw new HttpError(response.status, `HTTP ${response.status}`, code);
  }
  return (await response.json()) as User;
}

/** End one browser session by its own bearer, without cookies (best effort). */
async function endSessionWith(base: string, accessToken: string): Promise<void> {
  try {
    await fetch(`${base}/auth/logout`, {
      method: "POST",
      headers: { Authorization: `Bearer ${accessToken}` },
      credentials: "omit",
    });
  } catch (err) {
    console.warn("[Auth] Ending an abandoned session failed:", err instanceof Error ? err.message : err);
  }
}

/**
 * Browser: take over the access token of a session-creating answer (the
 * refresh token is an HttpOnly cookie) once its profile has loaded from
 * `base`, the server the answer came from. If the caller no longer wants it
 * (`stillWanted`, checked before and after the profile load) or the profile
 * cannot be loaded, the new session is ended instead and nothing is kept.
 */
export async function adoptBrowserAuth(
  response: AuthResponse,
  opts: { base?: string; stillWanted?: StillWanted } = {},
): Promise<AuthResult> {
  const base = opts.base ?? browserState.serverUrl;
  const wanted = opts.stillWanted ?? (() => true);
  let user: User;
  try {
    if (!wanted()) throw abandonedSignIn();
    user = await fetchMeWith(base, response.access_token);
    if (!wanted()) throw abandonedSignIn();
  } catch (err) {
    await endSessionWith(base, response.access_token);
    throw err;
  }
  // The token and its server together (Argon review 5 #1).
  setBrowserToken(base, response.access_token, response.expires_in);
  try {
    localStorage.setItem("serverUrl", browserState.serverUrl);
  } catch {
    // Storage blocked: this tab still knows the server.
  }
  scheduleTokenRefresh();
  const result: AuthResult = {
    user,
    setup_required: response.setup_required,
    password_change_required: response.password_change_required ?? false,
  };
  // Remembered so a caller that no longer wants it can end exactly this
  // session (`discardBrowserSession`); the result itself carries no token.
  adopted.set(result, { base, token: response.access_token });
  return result;
}

/** Sessions this tab adopted, by their result (browser). */
const adopted = new WeakMap<AuthResult, { base: string; token: string }>();

/**
 * End exactly the session `result` adopted (Codex r3 M2): with its own
 * bearer and no cookies, so a session installed meanwhile is never touched,
 * and its token is dropped here only while it is still the current one.
 * A result this tab did not adopt ends nothing.
 */
export async function discardBrowserSession(result: AuthResult): Promise<void> {
  const held = adopted.get(result);
  if (!held) return;
  adopted.delete(result);
  if (browserState.accessToken === held.token) clearBrowserTokens();
  await endSessionWith(held.base, held.token);
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
