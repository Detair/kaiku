/**
 * Login attempts (C27, SM1): "Sign in with your phone" (QR), the password and
 * OIDC sign-ins that wait for a phone confirmation, and the TOTP step.
 *
 * Browser: the collection secret is the `__Secure-kaiku_attempt` cookie, so
 * no `client_challenge` and no `verifier` are sent. Desktop: Rust draws the
 * verifier, sends `client_challenge` and keeps the verifier; the webview only
 * passes the attempt id (commands `login_attempt_*`).
 */

import {
  decodeLoginAttempt,
  decodeLoginStep,
  type LoginAttempt,
  type LoginStep,
} from "../types/login";
import {
  browserState,
  httpRequest,
  isTauri,
  loadNativeAuthInfo,
  type AuthResponse,
  type AuthResult,
} from "./common";
import {
  abandonedSignIn,
  adoptBrowserAuth,
  baseOf,
  invokeNative,
  useBrowserServer,
  type StillWanted,
} from "./preSession";
import { logout as transportLogout } from "./auth";

function attemptPath(attemptId: string, step: "poll" | "collect" | "totp" | "cancel"): string {
  return `/auth/login-attempts/${encodeURIComponent(attemptId)}/${step}`;
}

/** Start "Sign in with your phone": a QR attempt (`kind: "qr"`). */
export async function createQrAttempt(serverUrl: string): Promise<LoginAttempt> {
  if (isTauri()) {
    const raw = await invokeNative((invoke) =>
      invoke<unknown>("login_attempt_create", { serverUrl: baseOf(serverUrl) }),
    );
    return decodeLoginAttempt(raw);
  }
  useBrowserServer(serverUrl);
  return decodeLoginAttempt(await httpRequest<unknown>("POST", "/auth/login-attempts", {}));
}

/**
 * Username + password (F3). Never a session: 202 `LoginStep`, or 401
 * `INVALID_CREDENTIALS` (the neutral answer). `mfa_code` is gone (C27).
 */
export async function passwordLogin(
  serverUrl: string,
  username: string,
  password: string,
): Promise<LoginStep> {
  if (isTauri()) {
    const raw = await invokeNative((invoke) =>
      invoke<unknown>("login", {
        request: { server_url: baseOf(serverUrl), username, password },
      }),
    );
    return decodeLoginStep(raw);
  }
  useBrowserServer(serverUrl);
  return decodeLoginStep(await httpRequest<unknown>("POST", "/auth/login", { username, password }));
}

/** Long-poll the attempt (`wait` 0..25 s); a new `qr` means the code rotated. */
export async function pollAttempt(attemptId: string, wait: number): Promise<LoginAttempt> {
  const w = Math.max(0, Math.min(25, Math.round(wait)));
  if (isTauri()) {
    const raw = await invokeNative((invoke) =>
      invoke<unknown>("login_attempt_poll", { attemptId, wait: w }),
    );
    return decodeLoginAttempt(raw);
  }
  return decodeLoginAttempt(await httpRequest<unknown>("POST", attemptPath(attemptId, "poll"), { wait: w }));
}

/**
 * Collect an `approved` attempt: the session (probation) starts here, unless
 * `stillWanted()` is false by then (the user left or started another
 * sign-in): that session is ended and the call fails with 410
 * `LOGIN_ATTEMPT_ENDED {state: "cancelled"}`. Desktop: Rust makes the same
 * check against a cancel of the attempt.
 */
export async function collectAttempt(attemptId: string, stillWanted?: StillWanted): Promise<AuthResult> {
  if (isTauri()) {
    return nativeSessionStep(
      (invoke) => invoke<AuthResult>("login_attempt_collect", { attemptId }),
      stillWanted,
    );
  }
  const base = browserState.serverUrl;
  const response = await httpRequest<AuthResponse>("POST", attemptPath(attemptId, "collect"), {});
  return adoptBrowserAuth(response, { base, stillWanted });
}

/** The TOTP step of a password or OIDC attempt (F4); `stillWanted` as for collect. */
export async function attemptTotp(attemptId: string, code: string, stillWanted?: StillWanted): Promise<AuthResult> {
  if (isTauri()) {
    return nativeSessionStep(
      (invoke) => invoke<AuthResult>("login_attempt_totp", { attemptId, code }),
      stillWanted,
    );
  }
  const base = browserState.serverUrl;
  const response = await httpRequest<AuthResponse>("POST", attemptPath(attemptId, "totp"), { code });
  return adoptBrowserAuth(response, { base, stillWanted });
}

/**
 * Desktop collect or TOTP step under `stillWanted` (Argon review 5 #3): an
 * unwanted step starts nothing, and a session that arrives unwanted is ended
 * by its generation, never the session that may have replaced it.
 */
async function nativeSessionStep(
  run: (invoke: typeof import("@tauri-apps/api/core").invoke) => Promise<AuthResult>,
  stillWanted?: StillWanted,
): Promise<AuthResult> {
  if (stillWanted && !stillWanted()) throw abandonedSignIn();
  const result = await invokeNative(run);
  if (stillWanted && !stillWanted()) {
    await discardAdoptedSession(result);
    throw abandonedSignIn();
  }
  await loadNativeAuthInfo();
  return result;
}

/**
 * End exactly the session a sign-in step produced, when nobody wants it any
 * more (Codex review M2). Desktop: Rust signs out only if that session (its
 * generation) is still the current one; a newer session already ended it.
 * Browser: the adoption checks `stillWanted` right before it installs the
 * token, so the adopted session is the current one here.
 */
export async function discardAdoptedSession(result: AuthResult): Promise<void> {
  if (isTauri()) {
    if (typeof result.session_generation !== "number") return;
    await invokeNative((invoke) =>
      invoke<boolean>("login_attempt_discard_session", { generation: result.session_generation }),
    );
    await loadNativeAuthInfo();
    return;
  }
  await transportLogout();
}

/** Give the attempt up (the user left the screen, SM1). */
export async function cancelAttempt(attemptId: string): Promise<void> {
  if (isTauri()) {
    await invokeNative((invoke) => invoke<void>("login_attempt_cancel", { attemptId }));
    return;
  }
  await httpRequest<void>("POST", attemptPath(attemptId, "cancel"), {});
}
