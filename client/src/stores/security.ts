/**
 * Own-account security overview (`GET /api/me/security`, C27): assurance,
 * factor states, counts, the recovery contact, an open request and the
 * migration prompts. Refetched on `authenticators_changed` and
 * `account_request_updated` hints; never acted on from a hint alone.
 */

import { createSignal } from "solid-js";
import { getMeSecurity } from "@/lib/tauri/security";
import type { MeSecurity, SecurityPrompt } from "@/lib/types";

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

/** Banner prompts, highest priority first (`open_request` is the modal's). */
const PROMPT_ORDER: SecurityPrompt[] = [
  "secure_my_account",
  "set_up_phone",
  "add_passkey",
  "sign_in_with_passkey",
  "name_recovery_contact",
  "renew_totp",
  "renew_recovery_codes",
];

const SNOOZE_MS = 7 * 24 * 60 * 60 * 1000;
const snoozeKey = (p: SecurityPrompt) => `kaiku:prompt-snooze:${p}`;
const [snoozeTick, setSnoozeTick] = createSignal(0);

/** A store that fails (private mode, blocked site data) means "not snoozed". */
function isSnoozed(p: SecurityPrompt): boolean {
  try {
    const until = Number(localStorage.getItem(snoozeKey(p)));
    return Number.isFinite(until) && until > Date.now();
  } catch {
    return false;
  }
}

/** The one prompt the banner shows now, or null. */
export function currentPrompt(): SecurityPrompt | null {
  snoozeTick();
  const prompts = security()?.prompts ?? [];
  return PROMPT_ORDER.find((p) => prompts.includes(p) && !isSnoozed(p)) ?? null;
}

/** "Later": hide a prompt for 7 days in this browser. */
export function snoozePrompt(p: SecurityPrompt): void {
  try {
    localStorage.setItem(snoozeKey(p), String(Date.now() + SNOOZE_MS));
  } catch {
    // Not stored: the prompt simply comes back.
  }
  setSnoozeTick((n) => n + 1);
}
