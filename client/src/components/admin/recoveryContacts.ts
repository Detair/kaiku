/**
 * Who an admin may name as an account's recovery contact (C28): never an
 * admin, the account itself or the system account, and not a banned
 * account. The server refuses the rest with 400 `RECOVERY_CONTACT_INVALID`.
 */

import type { UserSummary } from "@/lib/types";

/** `users` without the accounts that can't be a contact, nor any id in `exclude`. */
export function contactCandidates(users: readonly UserSummary[] | undefined, exclude: readonly string[]): UserSummary[] {
  return (users ?? []).filter((u) => !u.is_banned && !u.is_admin && !u.is_system && !exclude.includes(u.id));
}
