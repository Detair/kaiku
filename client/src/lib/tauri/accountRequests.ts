/**
 * Account requests (C27, SM3): "I lost my phone / forgot my password"
 * (recovery) and "Secure my account" (bootstrap), then the request client
 * that waits, stages new sign-in methods and finishes.
 *
 * Browser: the request secret is the `__Secure-kaiku_request` cookie.
 * Desktop: Rust strips `request_secret` from the start answer, keeps it in the
 * keyring and adds `X-Kaiku-Request-Secret` to every request-client call.
 */

import {
  decodeAccountRequest,
  type AccountRequest,
  type BootstrapStart,
  type RecoveryStart,
  type TotpSetup,
} from "../types/login";
import { httpRequest, isTauri } from "./common";
import { baseOf, invokeNative, preSessionRequest, useBrowserServer } from "./preSession";
import { invokeCancellableOidc } from "./auth";

const CURRENT = "/auth/account-requests/current";

/** Start a recovery or bootstrap request (public; a session grants nothing here). */
export async function startAccountRequest(
  serverUrl: string,
  body: RecoveryStart | BootstrapStart,
): Promise<AccountRequest> {
  if (isTauri()) {
    const r = await invokeNative((invoke) =>
      invoke<{ request: unknown }>("account_request_start", { serverUrl: baseOf(serverUrl), body }),
    );
    return decodeAccountRequest(r?.request);
  }
  useBrowserServer(serverUrl);
  const r = await httpRequest<{ request: unknown }>("POST", "/auth/account-requests", body);
  return decodeAccountRequest(r?.request);
}

/**
 * The OIDC factor of a request. Browser: the provider URL for a popup, whose
 * page posts `oidc-request-callback {oidc_code}`. Desktop: Rust runs the
 * whole flow (C29 step 8 loopback delivery) and keeps the verifier; it adds it
 * to the start body itself.
 */
export async function requestOidcAuthorize(
  serverUrl: string,
  providerId: string,
  /** Desktop: aborting cancels the Rust flow (`oidc_cancel`). */
  signal?: AbortSignal,
): Promise<{ mode: "tauri"; oidcCode: string } | { mode: "browser"; url: string }> {
  if (isTauri()) {
    const r = await invokeCancellableOidc(
      (invoke, attemptId) =>
        invoke<{ oidc_code: string }>("request_oidc_factor", { serverUrl: baseOf(serverUrl), providerId, attemptId }),
      signal,
    );
    return { mode: "tauri", oidcCode: r.oidc_code };
  }
  useBrowserServer(serverUrl);
  const r = await httpRequest<{ url: string }>(
    "POST",
    `/auth/account-requests/oidc/authorize/${encodeURIComponent(providerId)}`,
  );
  return { mode: "browser", url: r.url };
}

/**
 * The request this client follows. Browser: read from `serverUrl`, the
 * server it was started on (Flash r3 p2 #3); the desktop's secret knows its
 * own server.
 */
export async function getCurrentRequest(serverUrl?: string): Promise<AccountRequest> {
  if (serverUrl && !isTauri()) useBrowserServer(serverUrl);
  return decodeAccountRequest(await preSessionRequest<unknown>("request", "GET", CURRENT));
}

/** Withdraw the request (the requester changed their mind). */
export async function withdrawRequest(): Promise<void> {
  await preSessionRequest<void>("request", "DELETE", CURRENT);
}

export async function setRequestPassword(password: string): Promise<void> {
  await preSessionRequest<void>("request", "PUT", `${CURRENT}/password`, { password });
}

export function startRequestTotp(): Promise<TotpSetup> {
  return preSessionRequest<TotpSetup>("request", "POST", `${CURRENT}/totp`);
}

export async function verifyRequestTotp(code: string): Promise<void> {
  await preSessionRequest<void>("request", "POST", `${CURRENT}/totp/verify`, { code });
}

export async function setRequestRecoveryContact(username: string): Promise<void> {
  await preSessionRequest<void>("request", "PUT", `${CURRENT}/recovery-contact`, { username });
}

/** Finish: the new recovery codes are in this answer only. No session is minted. */
export async function finishRequest(): Promise<{ request: AccountRequest; recovery_codes: string[] }> {
  const r = await preSessionRequest<{ request: unknown; recovery_codes?: string[] }>(
    "request",
    "POST",
    `${CURRENT}/finish`,
  );
  return { request: decodeAccountRequest(r?.request), recovery_codes: r?.recovery_codes ?? [] };
}

/** Browser only: the request's passkey (C29); `pending` until Finish. */
export function startRequestPasskey(label: string | null): Promise<{ ceremony_id: string; options: unknown }> {
  return httpRequest("POST", `${CURRENT}/passkey/start`, { label });
}

export function finishRequestPasskey(ceremonyId: string, credential: unknown): Promise<unknown> {
  return httpRequest("POST", `${CURRENT}/passkey/finish`, { ceremony_id: ceremonyId, credential });
}
