/**
 * "Set up this PC" (SM5), the PC's side: a chat-only session asks, the phone
 * grants. Every route needs the bearer session that started the attempt. The
 * phone's code is typed here (web) and never comes back from the server. On
 * the web the page sends a proof of its own with the code; only that proof
 * starts the passkey registration.
 */

import { httpRequest } from "./common";

/** SM5 states. */
export type PcSetupState =
  | "waiting"
  | "claimed"
  | "code"
  | "enrolled"
  | "completed"
  | "refused"
  | "reported"
  | "failed"
  | "cancelled"
  | "expired";

/** The states an attempt can still leave. */
export const LIVE_PC_SETUP_STATES: readonly PcSetupState[] = ["waiting", "claimed", "code", "enrolled"];

/** `GET /auth/pc-setups/{id}` and `/current`. */
export interface PcSetupStatus {
  attempt_id: string;
  state: PcSetupState;
  /** Why it ended: `session_ended`, `may_approve_off`, `refused`, `timeout`, … */
  reason: string | null;
  /** Only in a response that minted a new QR. */
  qr_payload?: string;
  rotates_at?: string;
  enrollment_id?: string;
}

/** `POST /auth/pc-setups` → 201. */
export interface PcSetupStarted {
  attempt_id: string;
  qr_payload: string;
  rotates_at: string;
}

/** `POST /auth/pc-setups/{id}/code`. */
export interface PcSetupCodeResult {
  state: PcSetupState;
  tries_left?: number;
  enrollment_id?: string;
}

/** `POST /auth/pc-setups/{id}/passkey/start`. */
export interface PcSetupPasskeyStart {
  ceremony_id: string;
  options: unknown;
}

const base = (id: string) => `/auth/pc-setups/${encodeURIComponent(id)}`;

/** Start (or restart: a live attempt of this session is cancelled). 403 `PC_SETUP_UNAVAILABLE`. */
export function startPcSetup(): Promise<PcSetupStarted> {
  return httpRequest<PcSetupStarted>("POST", "/auth/pc-setups", {});
}

/** The session's live attempt after a reload; 404 `APPROVAL_NOT_FOUND` when there is none. */
export function currentPcSetup(): Promise<PcSetupStatus> {
  return httpRequest<PcSetupStatus>("GET", "/auth/pc-setups/current");
}

/**
 * Long-poll (`wait` 0..25 s); a `qr_payload` means the code rotated. `signal`
 * closes the request: a long-poll holds a server slot until it ends.
 */
export function pollPcSetup(id: string, wait: number, signal?: AbortSignal): Promise<PcSetupStatus> {
  const w = Math.max(0, Math.min(25, Math.round(wait)));
  return httpRequest<PcSetupStatus>("GET", `${base(id)}?wait=${w}`, undefined, signal);
}

export async function cancelPcSetup(id: string): Promise<void> {
  await httpRequest<void>("DELETE", base(id));
}

/**
 * A fresh proof for one attempt's code: 32 random bytes, base64url without
 * padding. The page keeps it in memory only (never in the URL, in storage or
 * in a log) and sends it again with the passkey start.
 */
export function newPcSetupProof(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/**
 * Type the phone's 6 digits with the page's proof. A wrong code answers
 * `{state: "code", tries_left}`; a missing or malformed proof is 400.
 */
export function enterPcSetupCode(id: string, code: string, proof: string): Promise<PcSetupCodeResult> {
  return httpRequest<PcSetupCodeResult>("POST", `${base(id)}/code`, { code, proof });
}

/**
 * Browser only (the desktop app runs `passkey_pc_setup`): start the
 * registration of the attempt's passkey (`enrolled`) with the proof this page
 * sent with the right code. Without it, or with another, 409 `APPROVAL_STATE`.
 */
export function startPcSetupPasskey(id: string, proof: string | undefined): Promise<PcSetupPasskeyStart> {
  return httpRequest<PcSetupPasskeyStart>("POST", `${base(id)}/passkey/start`, proof ? { proof } : {});
}

/** Browser only: finish it. 409 `PASSKEY_LIMIT` / `PASSKEY_ALREADY_REGISTERED` leave the attempt `enrolled`. */
export function finishPcSetupPasskey(
  id: string,
  ceremonyId: string,
  credential: unknown,
  label: string | null,
): Promise<unknown> {
  return httpRequest<unknown>("POST", `${base(id)}/passkey/finish`, {
    ceremony_id: ceremonyId,
    credential,
    label,
  });
}
