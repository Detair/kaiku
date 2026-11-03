/**
 * The request client (C27, SM3): start a recovery or "Secure my account"
 * request, follow it while it waits (24 h or more), stage new sign-in
 * methods once it is ready, finish. No session is ever minted here.
 */

import { createSignal } from "solid-js";
import {
  finishRequest,
  getCurrentRequest,
  startAccountRequest,
  withdrawRequest,
} from "@/lib/tauri/accountRequests";
import { signInServer } from "@/lib/signInServer";
import { HttpError } from "@/lib/tauri/common";
import { isAuthenticated } from "@/stores/auth";
import type { AccountRequest, BootstrapStart, RecoveryStart } from "@/lib/types";

const [current, setCurrent] = createSignal<AccountRequest | null>(null);

/** The request this client follows, or null. */
export const request = current;

/** The account's own server when signed in, else the sign-in page's (Codex M3). */
export function requestServer(): string {
  return signInServer(isAuthenticated());
}

export async function startRecovery(r: RecoveryStart): Promise<void> {
  setCurrent(await startAccountRequest(requestServer(), r));
}

export async function startBootstrap(b: BootstrapStart): Promise<void> {
  setCurrent(await startAccountRequest(requestServer(), b));
}

export async function reloadRequest(): Promise<void> {
  try {
    setCurrent(await getCurrentRequest());
  } catch (err) {
    // The request (or its secret) is gone: show nothing of it any more.
    if (err instanceof HttpError && [401, 404, 410].includes(err.status)) setCurrent(null);
    throw err;
  }
}

/**
 * Withdraw: the secret (keyring or cookie) is gone afterwards, so the
 * request is not read again; it is marked withdrawn here (Argon review 6 #8).
 */
export async function withdraw(): Promise<void> {
  await withdrawRequest();
  setCurrent((prev) => (prev ? { ...prev, state: "withdrawn" } : null));
}

/** Finish: the new recovery codes, shown once. */
export async function finish(): Promise<string[]> {
  const r = await finishRequest();
  setCurrent(r.request);
  return r.recovery_codes;
}
