/**
 * Helpers for the admin create-account hand-over view.
 */

import { secureCopy } from "@/lib/clipboard";

/** Copy handover text (contains the temporary password) via the secure clipboard. */
export async function copyHandoverText(text: string): Promise<void> {
  await secureCopy(text, { other: "temporary_password" });
}

/**
 * Whether Escape or a backdrop click may dismiss the dialog. Once the account
 * exists the one-time password is only on screen, so only the explicit Close
 * button may close it.
 */
export function canDismissCreateAccount(accountCreated: boolean): boolean {
  return !accountCreated;
}
