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
import { getServerUrl } from "@/lib/tauri/common";
import type { AccountRequest, BootstrapStart, RecoveryStart } from "@/lib/types";

const [current, setCurrent] = createSignal<AccountRequest | null>(null);

/** The request this client follows, or null. */
export const request = current;

function serverUrl(): string {
  return getServerUrl() || (typeof window !== "undefined" ? window.location.origin : "");
}

export async function startRecovery(r: RecoveryStart): Promise<void> {
  setCurrent(await startAccountRequest(serverUrl(), r));
}

export async function startBootstrap(b: BootstrapStart): Promise<void> {
  setCurrent(await startAccountRequest(serverUrl(), b));
}

export async function reloadRequest(): Promise<void> {
  setCurrent(await getCurrentRequest());
}

export async function withdraw(): Promise<void> {
  await withdrawRequest();
  await reloadRequest();
}

/** Finish: the new recovery codes, shown once. */
export async function finish(): Promise<string[]> {
  const r = await finishRequest();
  setCurrent(r.request);
  return r.recovery_codes;
}
