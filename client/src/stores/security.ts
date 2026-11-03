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
/**
 * Bumped by `resetMeSecurity` (sign-out, a new session or account): a
 * request started before then is detached and its answer dropped, so one
 * account's overview never shows (or prompts) under the next one.
 */
let generation = 0;
let inFlight: { generation: number; promise: Promise<void> } | null = null;

/** The last loaded overview, or null before the first load. */
export const meSecurity = security;

/** Reload the overview (single flight per session). Failures keep the last value. */
export function refreshMeSecurity(): Promise<void> {
  if (inFlight && inFlight.generation === generation) return inFlight.promise;
  const mine = generation;
  const flight = {
    generation: mine,
    promise: getMeSecurity()
      .then((s) => {
        if (mine === generation) setSecurity(s);
      })
      .catch((err) => {
        console.warn("[Security] Loading the overview failed:", err instanceof Error ? err.message : err);
      })
      .finally(() => {
        if (inFlight === flight) inFlight = null;
      }),
  };
  inFlight = flight;
  return flight.promise;
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

/** Forget the overview (sign-out, new session); a running load is dropped. */
export function resetMeSecurity(): void {
  generation += 1;
  inFlight = null;
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
