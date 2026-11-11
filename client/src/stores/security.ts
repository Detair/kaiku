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
let inFlight: { generation: number; promise: Promise<void>; rerun: boolean } | null = null;

/** The last loaded overview, or null before the first load. */
export const meSecurity = security;

/**
 * C38: the "add a passkey for this computer" hint. Accounts that may not
 * confirm sign-ins (`may_approve` false) have no "Add a phone" in the phone
 * app, so they are told to ask the admin for a setup link instead.
 */
export function addPasskeyHintKey():
  | "auth.probation.addPasskeyHint"
  | "auth.probation.addPasskeyHintAskAdmin" {
  return security()?.may_approve === false
    ? "auth.probation.addPasskeyHintAskAdmin"
    : "auth.probation.addPasskeyHint";
}

/** C38: the same choice for the browser's empty passkey list. */
export function firstPasskeyHintKey():
  | "passkeys.settings.firstFromPhone"
  | "passkeys.settings.firstFromAdmin" {
  return security()?.may_approve === false
    ? "passkeys.settings.firstFromAdmin"
    : "passkeys.settings.firstFromPhone";
}

/**
 * Reload the overview (single flight per session). Failures keep the last
 * value. A reload asked for while one runs (a WS hint during the initial
 * load) runs once more afterwards, so a change the running request may have
 * read before is not missed (Argon review 6 #3).
 */
export function refreshMeSecurity(): Promise<void> {
  if (inFlight && inFlight.generation === generation) {
    inFlight.rerun = true;
    return inFlight.promise;
  }
  const mine = generation;
  const flight: { generation: number; promise: Promise<void>; rerun: boolean } = {
    generation: mine,
    rerun: false,
    promise: Promise.resolve(),
  };
  flight.promise = getMeSecurity()
    .then((s) => {
      if (mine === generation) setSecurity(s);
    })
    .catch((err) => {
      console.warn("[Security] Loading the overview failed:", err instanceof Error ? err.message : err);
    })
    .finally(() => {
      if (inFlight !== flight) return;
      inFlight = null;
      if (flight.rerun && mine === generation) void refreshMeSecurity();
    });
  inFlight = flight;
  return flight.promise;
}

const [requestHints, setRequestHints] = createSignal(0);

/** Bumped by every `account_request_updated` hint: request lists refetch on it. */
export const accountRequestHints = requestHints;

/** An `account_request_updated` hint arrived (C27; refetch, never act on it alone). */
export function noteAccountRequestHint(): void {
  setRequestHints((n) => n + 1);
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

/**
 * Open requests the user answered "That's right" for, by request id and
 * state (a `waiting` request that turns `ready` is shown again), for this
 * session only (Argon review 2 #3).
 */
const dismissedRequests = new Set<string>();
const [dismissTick, setDismissTick] = createSignal(0);

export function isOpenRequestDismissed(id: string, state: string): boolean {
  dismissTick();
  return dismissedRequests.has(`${id}:${state}`);
}

export function dismissOpenRequest(id: string, state: string): void {
  dismissedRequests.add(`${id}:${state}`);
  setDismissTick((n) => n + 1);
}

/** Show every open request again (a new session, a new app start). */
export function clearOpenRequestDismissals(): void {
  dismissedRequests.clear();
  setDismissTick((n) => n + 1);
}

/** Forget the overview (sign-out, new session); a running load is dropped. */
export function resetMeSecurity(): void {
  generation += 1;
  inFlight = null;
  setSecurity(null);
  setSignOutOthersOffered(false);
  clearOpenRequestDismissals();
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
/** Per account (`scope` = user id): one user's "Later" never hides another's prompt (Argon 6 #9). */
const snoozeKey = (p: SecurityPrompt, scope: string) => `kaiku:prompt-snooze:${scope}:${p}`;
const [snoozeTick, setSnoozeTick] = createSignal(0);

/** A store that fails (private mode, blocked site data) means "not snoozed". */
function isSnoozed(p: SecurityPrompt, scope: string): boolean {
  try {
    const until = Number(localStorage.getItem(snoozeKey(p, scope)));
    return Number.isFinite(until) && until > Date.now();
  } catch {
    return false;
  }
}

/** The one prompt the banner shows now for account `scope`, or null. */
export function currentPrompt(scope = ""): SecurityPrompt | null {
  snoozeTick();
  const prompts = security()?.prompts ?? [];
  return PROMPT_ORDER.find((p) => prompts.includes(p) && !isSnoozed(p, scope)) ?? null;
}

/** "Later": hide a prompt of account `scope` for 7 days in this browser. */
export function snoozePrompt(p: SecurityPrompt, scope = ""): void {
  try {
    localStorage.setItem(snoozeKey(p, scope), String(Date.now() + SNOOZE_MS));
  } catch {
    // Not stored: the prompt simply comes back.
  }
  setSnoozeTick((n) => n + 1);
}
