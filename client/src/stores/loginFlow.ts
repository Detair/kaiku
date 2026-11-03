/**
 * Sign-in flow on the PC (SM1, F2–F4): "Sign in with your phone", the
 * password or SSO sign-in that waits for the phone's confirmation, and the
 * TOTP step. Drives one login attempt by long-poll; the collection secret is
 * a cookie (browser) or stays in Rust (desktop), so nothing here ever holds a
 * verifier or a token.
 *
 * - While the step is `qr` or `confirm`, the attempt is polled with `wait` 25.
 *   A poll answer with `qr` replaces the shown code (same attempt; a
 *   rotation resets nothing). `approved` is collected exactly once.
 * - `denied`/`reported`/`cancelled`/`expired`, a 410 `LOGIN_ATTEMPT_ENDED` or
 *   a 404 `LOGIN_ATTEMPT_UNKNOWN` end the flow; so does the attempt's
 *   `expires_at` on the PC's clock.
 * - A failed poll (offline, 5xx, 429) keeps the attempt and retries after
 *   1 s, 2 s, then every 5 s.
 * - Leaving the screen cancels the attempt (SM1 "close the screen"); so does
 *   the attempt's expiry on the PC's clock and an SSO step that replaces it.
 * - Each attempt belongs to one run with a fixed server. A collect or TOTP
 *   answer for a run that is no longer the current one is never taken over:
 *   that session is ended (browser wrappers and Rust check before adopting;
 *   here once more afterwards). A failed collect is retried like a poll.
 */

import { createSignal } from "solid-js";
import {
  attemptTotp,
  cancelAttempt,
  collectAttempt,
  createQrAttempt,
  logout as endTransportSession,
  pollAttempt,
} from "@/lib/tauri";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { loginErrorText } from "@/lib/authErrors";
import type { LoginAttempt, LoginStep, User } from "@/lib/types";
import { completeSignIn, login as passwordSignIn } from "@/stores/auth";

export { loginErrorText };

export type EndedState = "denied" | "reported" | "cancelled" | "expired";
export type FlowVia = "qr" | "password" | "oidc";

export type LoginFlow =
  | { step: "form" }
  | { step: "qr"; attempt: LoginAttempt; qrPayload: string }
  | { step: "confirm"; attempt: LoginAttempt; qrPayload: string; via: "password" | "oidc" }
  | { step: "totp"; attempt: LoginAttempt; from: "totp_required" | "phone_not_at_hand" }
  | { step: "ended"; state: EndedState; via: FlowVia };

/** Long-poll wait in seconds (C27: 0..25). */
const POLL_WAIT_S = 25;
/** Never poll more often than this, even if a server answers at once. */
const MIN_POLL_INTERVAL_MS = 1_000;
/** Retry delays after failed polls and collects: 1 s, 2 s, then every 5 s. */
const RETRY_DELAYS_MS = [1_000, 2_000, 5_000];
/** An approved attempt can be collected for 60 s (C27); retries stop then. */
const COLLECT_WINDOW_MS = 60_000;

/** Largest delay `setTimeout` keeps (2^31 - 1 ms). */
const MAX_TIMER_MS = 2_147_483_647;

const ENDED: readonly EndedState[] = ["denied", "reported", "cancelled", "expired"];

const [flow, setFlow] = createSignal<LoginFlow>({ step: "form" });
const [flowError, setFlowError] = createSignal<string | null>(null);
const [flowBusy, setFlowBusy] = createSignal(false);

/** The current step of the sign-in flow. */
export const loginFlow = flow;
/** A sentence about the last failure of the flow, or null. */
export const loginFlowError = flowError;
/** A sign-in step is being sent (TOTP submit, collect). */
export const loginFlowBusy = flowBusy;

/**
 * One run of the flow: its server never changes. A new run object replaces
 * the current one whenever the flow changes attempt or ends, so older loops
 * and late answers can tell they are obsolete (`isLive`).
 */
interface Run {
  readonly serverUrl: string;
}

let current: Run = { serverUrl: "" };
let via: FlowVia = "qr";
/** The server the next SSO step belongs to (`setFlowServer`). */
let nextServer = "";
let expiryTimer: ReturnType<typeof setTimeout> | null = null;

const isLive = (r: Run): boolean => r === current;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function clearExpiry(): void {
  if (expiryTimer) clearTimeout(expiryTimer);
  expiryTimer = null;
}

/** Start a new run on `serverUrl`; every older run becomes obsolete. */
function startRun(serverUrl: string): Run {
  clearExpiry();
  setFlowBusy(false);
  current = { serverUrl };
  return current;
}

/** Stop the current run without telling the server (it is replaced or over). */
function stopRun(): void {
  startRun(current.serverUrl);
}

/** End the flow for good (the attempt cannot sign anyone in any more). */
function end(state: EndedState): void {
  stopRun();
  setFlow({ step: "ended", state, via });
}

/** Tell the server (and Rust) an attempt is given up; failures only logged. */
function dropAttempt(attemptId: string): void {
  void cancelAttempt(attemptId).catch((err: unknown) => {
    console.warn("[LoginFlow] Cancelling the attempt failed:", err instanceof Error ? err.message : err);
  });
}

/**
 * A session was made for a run the user already left (desktop: Rust adopted
 * it just before the cancel arrived). It is ended, never shown.
 */
async function discardSession(): Promise<void> {
  try {
    await endTransportSession();
  } catch (err) {
    console.warn("[LoginFlow] Ending an abandoned session failed:", err instanceof Error ? err.message : err);
  }
}

/** The attempt runs out on the PC's clock too (the long-poll may hang). */
function armExpiry(attempt: LoginAttempt, r: Run): void {
  clearExpiry();
  const at = Date.parse(attempt.expires_at);
  if (Number.isNaN(at)) return;
  // Timers overflow past 2^31-1 ms and would fire at once; attempts live
  // at most 10 minutes anyway.
  const delay = Math.min(Math.max(0, at - Date.now()), MAX_TIMER_MS);
  expiryTimer = setTimeout(() => {
    if (!isLive(r)) return;
    end("expired");
    // The verifier (desktop) and the attempt go now, not at the next call.
    dropAttempt(attempt.attempt_id);
  }, delay);
}

/** The ended state a refused attempt call reports, or null if it may be retried. */
function endedBy(err: unknown): EndedState | null {
  if (!(err instanceof HttpError)) return null;
  if (codeIs(err.code, "LOGIN_ATTEMPT_UNKNOWN") || err.status === 404) return "expired";
  if (codeIs(err.code, "LOGIN_ATTEMPT_ENDED") || err.status === 410) {
    const state = err.details.state;
    return ENDED.includes(state as EndedState) ? (state as EndedState) : "expired";
  }
  return null;
}

/** Transient: offline, 5xx, rate limited. Anything else stops the flow. */
function retryable(err: unknown): boolean {
  if (err instanceof TypeError) return true;
  return err instanceof HttpError && (err.status === 429 || err.status >= 500 || err.status === 0);
}

function currentAttempt(): LoginAttempt | null {
  const f = flow();
  return f.step === "qr" || f.step === "confirm" || f.step === "totp" ? f.attempt : null;
}

/** Back to the form with a sentence about `err`; the attempt is given up. */
function failRun(err: unknown, attemptId: string | null): void {
  stopRun();
  if (attemptId) dropAttempt(attemptId);
  setFlowError(loginErrorText(err));
  setFlow({ step: "form" });
}

/**
 * Collect an approved attempt (exactly once successfully). Transient
 * failures are retried within the collect window; `false` means the attempt
 * is not ready after all and polling goes on.
 */
async function collect(r: Run, attemptId: string): Promise<boolean> {
  // The attempt is approved: its expiry no longer matters, and a session
  // that the server already made must never be dropped by the timer.
  clearExpiry();
  setFlowBusy(true);
  const giveUpAt = Date.now() + COLLECT_WINDOW_MS;
  let failures = 0;
  try {
    for (;;) {
      try {
        const result = await collectAttempt(attemptId, () => isLive(r));
        if (!isLive(r)) {
          await discardSession();
          return true;
        }
        stopRun();
        await completeSignIn(r.serverUrl, result);
        setFlow({ step: "form" });
        return true;
      } catch (err) {
        if (!isLive(r)) return true;
        const ended = endedBy(err);
        if (ended) {
          end(ended);
          return true;
        }
        if (err instanceof HttpError && codeIs(err.code, "LOGIN_ATTEMPT_NOT_READY")) return false;
        if (!retryable(err) || Date.now() >= giveUpAt) {
          failRun(err, attemptId);
          return true;
        }
        await sleep(RETRY_DELAYS_MS[Math.min(failures, RETRY_DELAYS_MS.length - 1)]);
        failures += 1;
        if (!isLive(r)) return true;
      }
    }
  } finally {
    if (isLive(r)) setFlowBusy(false);
  }
}

/** Apply one poll answer; `true` once the loop must stop. */
async function applyPolled(r: Run, next: LoginAttempt): Promise<boolean> {
  if (next.state === "approved") return collect(r, next.attempt_id);
  if (next.state === "consumed") {
    end("expired");
    return true;
  }
  if (ENDED.includes(next.state as EndedState)) {
    end(next.state as EndedState);
    return true;
  }
  const f = flow();
  if (f.step === "qr") {
    setFlow({ ...f, attempt: { ...next, qr: next.qr ?? f.attempt.qr }, qrPayload: next.qr?.payload ?? f.qrPayload });
  } else if (f.step === "confirm") {
    setFlow({ ...f, attempt: { ...next, qr: next.qr ?? f.attempt.qr }, qrPayload: next.qr?.payload ?? f.qrPayload });
  }
  return false;
}

async function pollLoop(r: Run): Promise<void> {
  let failures = 0;
  while (isLive(r)) {
    const attempt = currentAttempt();
    const f = flow();
    if (!attempt || (f.step !== "qr" && f.step !== "confirm")) return;
    const started = Date.now();
    let next: LoginAttempt;
    try {
      next = await pollAttempt(attempt.attempt_id, POLL_WAIT_S);
    } catch (err) {
      if (!isLive(r)) return;
      const ended = endedBy(err);
      if (ended) {
        end(ended);
        return;
      }
      if (!retryable(err)) {
        failRun(err, null);
        return;
      }
      const delay = RETRY_DELAYS_MS[Math.min(failures, RETRY_DELAYS_MS.length - 1)];
      failures += 1;
      await sleep(delay);
      continue;
    }
    failures = 0;
    if (!isLive(r)) return;
    if (await applyPolled(r, next)) return;
    const elapsed = Date.now() - started;
    if (elapsed < MIN_POLL_INTERVAL_MS) await sleep(MIN_POLL_INTERVAL_MS - elapsed);
  }
}

/**
 * Show a `LoginStep` (password or SSO answer, F3) as a new run on
 * `serverUrl` (default: the server named by `setFlowServer`). A different
 * live attempt it replaces is cancelled.
 */
export function applyLoginStep(step: LoginStep, from: "password" | "oidc", serverUrl: string = nextServer): void {
  const replaced = currentAttempt();
  const r = startRun(serverUrl);
  if (replaced && replaced.attempt_id !== step.attempt.attempt_id) dropAttempt(replaced.attempt_id);
  via = from;
  setFlowError(null);
  armExpiry(step.attempt, r);
  if (step.status === "totp_required") {
    setFlow({ step: "totp", attempt: step.attempt, from: "totp_required" });
    return;
  }
  setFlow({
    step: "confirm",
    attempt: step.attempt,
    qrPayload: step.attempt.qr?.payload ?? "",
    via: from,
  });
  void pollLoop(r);
}

/** "Sign in with your phone" (F2): a new QR attempt. */
export async function startPhoneSignIn(url: string): Promise<void> {
  await leaveFlow();
  via = "qr";
  const r = startRun(url);
  setFlowBusy(true);
  try {
    const attempt = await createQrAttempt(url);
    if (!isLive(r)) {
      dropAttempt(attempt.attempt_id);
      return;
    }
    armExpiry(attempt, r);
    setFlow({ step: "qr", attempt, qrPayload: attempt.qr?.payload ?? "" });
    void pollLoop(r);
  } catch (err) {
    if (isLive(r)) setFlowError(loginErrorText(err));
  } finally {
    if (isLive(r)) setFlowBusy(false);
  }
}

/**
 * Username + password (F3). The auth store sends it and words a refusal
 * (`authState.error`); the answer is never a session.
 */
export async function submitPassword(url: string, username: string, password: string): Promise<void> {
  await leaveFlow();
  const r = startRun(url);
  const step = await passwordSignIn(url, username, password);
  if (!isLive(r)) {
    // Left (or submitted again) while the password was checked.
    dropAttempt(step.attempt.attempt_id);
    return;
  }
  applyLoginStep(step, "password", url);
}

/** Name the server of an SSO sign-in whose `LoginStep` arrives later. */
export function setFlowServer(url: string): void {
  nextServer = url;
}

/** "Phone not at hand?": the same attempt continues with TOTP (F3 step 5). */
export function usePhoneNotAtHand(): void {
  const f = flow();
  // While an approved attempt is being collected, the session is on its way.
  if (f.step !== "confirm" || !f.attempt.totp_available || flowBusy()) return;
  const attempt = f.attempt;
  const r = startRun(current.serverUrl);
  armExpiry(attempt, r);
  setFlowError(null);
  setFlow({ step: "totp", attempt, from: "phone_not_at_hand" });
}

/** The TOTP step (F4). A wrong code keeps the step and says so. */
export async function submitTotp(code: string): Promise<User> {
  const f = flow();
  if (f.step !== "totp") throw new Error("No TOTP step");
  const r = current;
  setFlowBusy(true);
  setFlowError(null);
  try {
    const result = await attemptTotp(f.attempt.attempt_id, code, () => isLive(r));
    if (!isLive(r)) {
      await discardSession();
      throw new HttpError(410, "LOGIN_ATTEMPT_ENDED", "LOGIN_ATTEMPT_ENDED", { state: "cancelled" });
    }
    stopRun();
    const user = await completeSignIn(r.serverUrl, result);
    setFlow({ step: "form" });
    return user;
  } catch (err) {
    if (isLive(r)) {
      const ended = endedBy(err);
      if (ended) end(ended);
      else setFlowError(loginErrorText(err));
    }
    throw err;
  } finally {
    if (isLive(r)) setFlowBusy(false);
  }
}

/** "Try again" after an ended attempt: a new QR, or back to the form. */
export async function retryLoginFlow(): Promise<void> {
  const f = flow();
  if (f.step === "ended" && f.via === "qr") {
    await startPhoneSignIn(current.serverUrl);
    return;
  }
  stopRun();
  setFlowError(null);
  setFlow({ step: "form" });
}

/** Leave the screen or go back: cancel a live attempt (SM1). */
export async function leaveFlow(): Promise<void> {
  const attempt = currentAttempt();
  stopRun();
  setFlowError(null);
  setFlow({ step: "form" });
  if (attempt) {
    try {
      await cancelAttempt(attempt.attempt_id);
    } catch (err) {
      console.warn("[LoginFlow] Cancelling the attempt failed:", err instanceof Error ? err.message : err);
    }
  }
}
