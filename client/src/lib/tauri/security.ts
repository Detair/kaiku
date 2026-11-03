/**
 * Own-account security (C26–C28): overview and prompts, the security log,
 * "That wasn't me", key devices, recovery codes, password, recovery contact
 * and the account requests the caller may cancel. All bearer calls; the
 * guarded ones need a full session and a step-up grant (`withStepUp`).
 */

import {
  decodeAccountRequestSummary,
  decodeKeyDevice,
  decodeMeSecurity,
  decodeSecurityEvent,
  type AccountRequestSummary,
  type ContactForEntry,
  type CursorPaginatedResponse,
  type KeyDevice,
  type MeSecurity,
  type RecoveryContact,
  type RepudiateResult,
  type SecurityEvent,
} from "../types/login";
import { httpRequest, isTauri } from "./common";
import { nativeSessionRequest } from "./auth";

/** A fresh second factor for owner rule part 2 (B.2): cancel or repudiate. */
export type Part2Factor = { type: "totp" | "recovery_code"; code: string };

function part2Body(part2: Part2Factor | null): { factor: Part2Factor } | null {
  return part2 ? { factor: { type: part2.type, code: part2.code } } : null;
}

function query(params: Record<string, string | number | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== "") q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : "";
}

export async function getMeSecurity(): Promise<MeSecurity> {
  return decodeMeSecurity(await httpRequest<unknown>("GET", "/api/me/security"));
}

export function decodeEventPage(raw: unknown): CursorPaginatedResponse<SecurityEvent> {
  const page = (raw ?? {}) as { items?: unknown[]; has_more?: unknown; next_cursor?: unknown };
  return {
    items: (Array.isArray(page.items) ? page.items : []).map(decodeSecurityEvent),
    has_more: page.has_more === true,
    next_cursor: typeof page.next_cursor === "string" ? page.next_cursor : null,
  };
}

/** The security log, newest first; `before` = the oldest event id seen. */
export async function listSecurityEvents(
  before?: string,
  limit?: number,
): Promise<CursorPaginatedResponse<SecurityEvent>> {
  const raw = await httpRequest<unknown>("GET", `/api/me/security-events${query({ before, limit })}`);
  return decodeEventPage(raw);
}

/** "That wasn't me": part 1 by a step-up grant, part 2 with a fresh factor. */
export async function repudiateEvent(id: string, part2: Part2Factor | null): Promise<RepudiateResult> {
  const r = await httpRequest<RepudiateResult>(
    "POST",
    `/api/me/security-events/${encodeURIComponent(id)}/repudiate`,
    { part2: part2Body(part2) },
  );
  return { ...r, event: decodeSecurityEvent(r?.event) };
}

export async function listKeyDevices(): Promise<KeyDevice[]> {
  const r = await httpRequest<{ devices?: unknown[] }>("GET", "/api/approval-devices");
  return (r?.devices ?? []).map(decodeKeyDevice);
}

/** Needs the grant `key_device.remove`/id; 409 `LAST_STRONG_AUTHENTICATOR`. */
export async function removeKeyDevice(id: string): Promise<KeyDevice> {
  return decodeKeyDevice(await httpRequest<unknown>("DELETE", `/api/approval-devices/${encodeURIComponent(id)}`));
}

export async function getRecoveryCodesLeft(): Promise<number> {
  const r = await httpRequest<{ left?: number }>("GET", "/auth/recovery-codes/count");
  return typeof r?.left === "number" ? r.left : 0;
}

/** New recovery codes (shown once); grant `recovery_codes.create`. */
export async function createRecoveryCodes(): Promise<string[]> {
  const r = await httpRequest<{ recovery_codes?: string[] }>("POST", "/auth/recovery-codes");
  return r?.recovery_codes ?? [];
}

/**
 * Set or change the password (grant `password.set`); 400 `PASSWORD_REJECTED
 * {reason}`. Desktop: through Rust's session request like every
 * `/auth/me/password` call so far.
 */
export async function setPassword(newPassword: string): Promise<void> {
  const body = { new_password: newPassword };
  if (isTauri()) {
    await nativeSessionRequest<unknown>("POST", "/auth/me/password", body);
    return;
  }
  await httpRequest<unknown>("POST", "/auth/me/password", body);
}

/** Remove the password (grant `password.remove`). */
export async function removePassword(): Promise<void> {
  await httpRequest<void>("DELETE", "/auth/me/password");
}

/** Name a recovery contact (grant `recovery_contact.set`/user id). */
export async function setRecoveryContact(userId: string): Promise<RecoveryContact> {
  return httpRequest<RecoveryContact>("PUT", "/api/me/recovery-contact", { user_id: userId });
}

/** Remove the recovery contact (grant `recovery_contact.set`/`""`). */
export async function removeRecoveryContact(): Promise<void> {
  await httpRequest<void>("DELETE", "/api/me/recovery-contact");
}

/** People who named the caller as their recovery contact. */
export async function listContactFor(): Promise<ContactForEntry[]> {
  const r = await httpRequest<{ accounts?: ContactForEntry[] }>("GET", "/api/me/recovery-contact-for");
  return r?.accounts ?? [];
}

/** "I will help <name>" (full session, no step-up). */
export async function confirmContactFor(userId: string): Promise<void> {
  await httpRequest<void>("POST", `/api/me/recovery-contact-for/${encodeURIComponent(userId)}/confirm`);
}

/** Open requests on the caller's account and those the caller may cancel as contact. */
export async function listMyAccountRequests(): Promise<AccountRequestSummary[]> {
  const r = await httpRequest<{ requests?: unknown[] }>("GET", "/api/me/account-requests");
  return (r?.requests ?? []).map(decodeAccountRequestSummary);
}

/** Cancel a request: grant `request.cancel`/id, or owner rule part 2. */
export async function cancelAccountRequest(
  id: string,
  reason: string | null,
  part2: Part2Factor | null,
): Promise<AccountRequestSummary> {
  const r = await httpRequest<unknown>("POST", `/api/account-requests/${encodeURIComponent(id)}/cancel`, {
    reason,
    part2: part2Body(part2),
  });
  return decodeAccountRequestSummary(r);
}

/**
 * `GET /auth/reauth/methods` in its C27 form: can this session step up with
 * a passkey, or with a key device's `device_stepup`? (A PC session never
 * signs `device_stepup`; `device` tells the UI to point to the phone.)
 */
export async function getStepUpMethods(): Promise<{ passkey: boolean; device: boolean }> {
  const r = await httpRequest<{ passkey?: unknown; device?: unknown }>("GET", "/auth/reauth/methods");
  return { passkey: r?.passkey === true, device: r?.device === true };
}
