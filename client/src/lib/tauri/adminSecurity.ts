/**
 * Admin side of the v1.0 login (C26–C28): "Remove device now", `may_approve`,
 * credential states, a user's recovery contact, open account requests and the
 * admin security log. Every change needs a full session and a step-up grant
 * whose target is the account or authenticator (`withStepUp`). Issuing setup
 * codes is done from the admin's own phone only (coordinator call 1).
 */

import {
  decodeAccountRequestSummary,
  type AccountRequestSummary,
  type CursorPaginatedResponse,
  type SecurityEvent,
} from "../types/login";
import { httpRequest } from "./common";
import { decodeEventPage } from "./security";

const user = (userId: string) => `/api/admin/users/${encodeURIComponent(userId)}`;

/** "Remove device now" (grant `admin.authenticator.remove`/id); 403 `ADMIN_TARGET_IS_ADMIN`. */
export async function adminRemoveAuthenticator(userId: string, authenticatorId: string): Promise<void> {
  await httpRequest<void>("POST", `${user(userId)}/authenticators/${encodeURIComponent(authenticatorId)}/remove`);
}

/** Grant `admin.may_approve`/user id. */
export async function adminSetMayApprove(userId: string, mayApprove: boolean): Promise<void> {
  await httpRequest<unknown>("PUT", `${user(userId)}/may-approve`, { may_approve: mayApprove });
}

export interface FactorStates {
  password?: "active" | "compromised";
  totp?: "active" | "compromised";
}

/** "Mark as unsafe" / "Allow again" (grant `admin.factor.set_state`/user id). */
export async function adminSetFactors(userId: string, states: FactorStates): Promise<void> {
  await httpRequest<unknown>("PUT", `${user(userId)}/factors`, states);
}

/** Grant `admin.recovery_contact.set`/user id; non-admin accounts only. */
export async function adminSetRecoveryContact(userId: string, contactId: string): Promise<void> {
  await httpRequest<unknown>("PUT", `${user(userId)}/recovery-contact`, { user_id: contactId });
}

/** `GET /api/admin/security-events?user_id=` (elevated). */
export async function listAdminSecurityEvents(
  userId: string,
  before?: string,
  limit?: number,
): Promise<CursorPaginatedResponse<SecurityEvent>> {
  const q = new URLSearchParams({ user_id: userId });
  if (before) q.set("before", before);
  if (limit) q.set("limit", String(limit));
  return decodeEventPage(await httpRequest<unknown>("GET", `/api/admin/security-events?${q.toString()}`));
}

/** `GET /api/admin/account-requests?state=open` (elevated). */
export async function listOpenAccountRequests(): Promise<AccountRequestSummary[]> {
  const r = await httpRequest<{ requests?: unknown[] }>("GET", "/api/admin/account-requests?state=open");
  return (r?.requests ?? []).map(decodeAccountRequestSummary);
}
