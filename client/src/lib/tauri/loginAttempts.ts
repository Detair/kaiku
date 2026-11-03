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
import { httpRequest, isTauri, loadNativeAuthInfo, type AuthResponse, type AuthResult } from "./common";
import { adoptBrowserAuth, baseOf, invokeNative, useBrowserServer } from "./preSession";

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

/** Collect an `approved` attempt: the session (probation) starts here. */
export async function collectAttempt(attemptId: string): Promise<AuthResult> {
  if (isTauri()) {
    const result = await invokeNative((invoke) =>
      invoke<AuthResult>("login_attempt_collect", { attemptId }),
    );
    await loadNativeAuthInfo();
    return result;
  }
  const response = await httpRequest<AuthResponse>("POST", attemptPath(attemptId, "collect"), {});
  return adoptBrowserAuth(response);
}

/** The TOTP step of a password or OIDC attempt (F4). */
export async function attemptTotp(attemptId: string, code: string): Promise<AuthResult> {
  if (isTauri()) {
    const result = await invokeNative((invoke) =>
      invoke<AuthResult>("login_attempt_totp", { attemptId, code }),
    );
    await loadNativeAuthInfo();
    return result;
  }
  const response = await httpRequest<AuthResponse>("POST", attemptPath(attemptId, "totp"), { code });
  return adoptBrowserAuth(response);
}

/** Give the attempt up (the user left the screen, SM1). */
export async function cancelAttempt(attemptId: string): Promise<void> {
  if (isTauri()) {
    await invokeNative((invoke) => invoke<void>("login_attempt_cancel", { attemptId }));
    return;
  }
  await httpRequest<void>("POST", attemptPath(attemptId, "cancel"), {});
}
