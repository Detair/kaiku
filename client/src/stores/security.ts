/**
 * Own-account security overview (`GET /api/me/security`, C27): assurance,
 * factor states, counts, the recovery contact, an open request and the
 * migration prompts. Refetched on `authenticators_changed` and
 * `account_request_updated` hints; never acted on from a hint alone.
 */

import { createSignal } from "solid-js";
import { getMeSecurity } from "@/lib/tauri/security";
import type { MeSecurity } from "@/lib/types";

const [security, setSecurity] = createSignal<MeSecurity | null>(null);
let inFlight: Promise<void> | null = null;

/** The last loaded overview, or null before the first load. */
export const meSecurity = security;

/** Reload the overview (single flight). Failures keep the last value. */
export function refreshMeSecurity(): Promise<void> {
  inFlight ??= getMeSecurity()
    .then((s) => {
      setSecurity(s);
    })
    .catch((err) => {
      console.warn("[Security] Loading the overview failed:", err instanceof Error ? err.message : err);
    })
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

const [signOutOthersOffered, setSignOutOthersOffered] = createSignal(false);

/** Whether the "Sign out everywhere else?" prompt is open. */
export const signOutOthersPrompt = signOutOthersOffered;

/**
 * After any change of a sign-in method, offer to end every other session
 * (ASVS 7.4.3). The prompt runs `withStepUp(sessions.end/others)`.
 */
export function offerSignOutOthers(): void {
  setSignOutOthersOffered(true);
}

export function closeSignOutOthers(): void {
  setSignOutOthersOffered(false);
}

/** Forget the overview (sign-out). */
export function resetMeSecurity(): void {
  setSecurity(null);
  setSignOutOthersOffered(false);
}
