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
 * - Leaving the screen cancels the attempt (SM1 "close the screen").
 */

import { createSignal } from "solid-js";
import {
  attemptTotp,
  cancelAttempt,
  collectAttempt,
  createQrAttempt,
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
/** Retry delays after failed polls: 1 s, 2 s, then every 5 s. */
const RETRY_DELAYS_MS = [1_000, 2_000, 5_000];

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

/** Bumped whenever the flow changes attempt or ends: older loops stop. */
let run = 0;
let via: FlowVia = "qr";
let serverUrl = "";
let expiryTimer: ReturnType<typeof setTimeout> | null = null;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function clearExpiry(): void {
  if (expiryTimer) clearTimeout(expiryTimer);
  expiryTimer = null;
}

/** End the flow for good (the attempt cannot sign anyone in any more). */
function end(state: EndedState): void {
  run += 1;
  clearExpiry();
  setFlowBusy(false);
  setFlow({ step: "ended", state, via });
}

/** The attempt runs out on the PC's clock too (the long-poll may hang). */
function armExpiry(attempt: LoginAttempt, myRun: number): void {
  clearExpiry();
  const at = Date.parse(attempt.expires_at);
  if (Number.isNaN(at)) return;
  expiryTimer = setTimeout(() => {
    if (myRun === run) end("expired");
  }, Math.max(0, at - Date.now()));
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

async function collect(myRun: number, attemptId: string): Promise<void> {
  setFlowBusy(true);
  try {
    const result = await collectAttempt(attemptId);
    if (myRun !== run) return;
    run += 1;
    clearExpiry();
    await completeSignIn(serverUrl, result);
    setFlow({ step: "form" });
  } catch (err) {
    if (myRun !== run) return;
    const ended = endedBy(err);
    if (ended) {
      end(ended);
      return;
    }
    run += 1;
    clearExpiry();
    setFlowError(loginErrorText(err));
    setFlow({ step: "form" });
  } finally {
    setFlowBusy(false);
  }
}

/** Apply one poll answer; `true` once the loop must stop. */
async function applyPolled(myRun: number, next: LoginAttempt): Promise<boolean> {
  if (next.state === "approved") {
    await collect(myRun, next.attempt_id);
    return true;
  }
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

async function pollLoop(myRun: number): Promise<void> {
  let failures = 0;
  while (myRun === run) {
    const attempt = currentAttempt();
    const f = flow();
    if (!attempt || (f.step !== "qr" && f.step !== "confirm")) return;
    const started = Date.now();
    let next: LoginAttempt;
    try {
      next = await pollAttempt(attempt.attempt_id, POLL_WAIT_S);
    } catch (err) {
      if (myRun !== run) return;
      const ended = endedBy(err);
      if (ended) {
        end(ended);
        return;
      }
      if (!retryable(err)) {
        run += 1;
        clearExpiry();
        setFlowError(loginErrorText(err));
        setFlow({ step: "form" });
        return;
      }
      const delay = RETRY_DELAYS_MS[Math.min(failures, RETRY_DELAYS_MS.length - 1)];
      failures += 1;
      await sleep(delay);
      continue;
    }
    failures = 0;
    if (myRun !== run) return;
    if (await applyPolled(myRun, next)) return;
    const elapsed = Date.now() - started;
    if (elapsed < MIN_POLL_INTERVAL_MS) await sleep(MIN_POLL_INTERVAL_MS - elapsed);
  }
}

/** Stop the current attempt without telling the server (it is replaced or over). */
function stopRun(): void {
  run += 1;
  clearExpiry();
  setFlowBusy(false);
}

/** Show a `LoginStep` (password or SSO answer, F3). */
export function applyLoginStep(step: LoginStep, from: "password" | "oidc"): void {
  stopRun();
  via = from;
  setFlowError(null);
  const myRun = run;
  armExpiry(step.attempt, myRun);
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
  void pollLoop(myRun);
}

/** "Sign in with your phone" (F2): a new QR attempt. */
export async function startPhoneSignIn(url: string): Promise<void> {
  await leaveFlow();
  serverUrl = url;
  via = "qr";
  const myRun = run;
  setFlowBusy(true);
  try {
    const attempt = await createQrAttempt(url);
    if (myRun !== run) {
      void cancelAttempt(attempt.attempt_id).catch(() => undefined);
      return;
    }
    armExpiry(attempt, myRun);
    setFlow({ step: "qr", attempt, qrPayload: attempt.qr?.payload ?? "" });
    void pollLoop(myRun);
  } catch (err) {
    if (myRun === run) setFlowError(loginErrorText(err));
  } finally {
    if (myRun === run) setFlowBusy(false);
  }
}

/**
 * Username + password (F3). The auth store sends it and words a refusal
 * (`authState.error`); the answer is never a session.
 */
export async function submitPassword(url: string, username: string, password: string): Promise<void> {
  await leaveFlow();
  serverUrl = url;
  const step = await passwordSignIn(url, username, password);
  applyLoginStep(step, "password");
}

/** Remember the server of an SSO sign-in whose `LoginStep` arrives later. */
export function setFlowServer(url: string): void {
  serverUrl = url;
}

/** "Phone not at hand?": the same attempt continues with TOTP (F3 step 5). */
export function usePhoneNotAtHand(): void {
  const f = flow();
  if (f.step !== "confirm" || !f.attempt.totp_available) return;
  const attempt = f.attempt;
  stopRun();
  const myRun = run;
  armExpiry(attempt, myRun);
  setFlowError(null);
  setFlow({ step: "totp", attempt, from: "phone_not_at_hand" });
}

/** The TOTP step (F4). A wrong code keeps the step and says so. */
export async function submitTotp(code: string): Promise<User> {
  const f = flow();
  if (f.step !== "totp") throw new Error("No TOTP step");
  const myRun = run;
  setFlowBusy(true);
  setFlowError(null);
  try {
    const result = await attemptTotp(f.attempt.attempt_id, code);
    stopRun();
    const user = await completeSignIn(serverUrl, result);
    setFlow({ step: "form" });
    return user;
  } catch (err) {
    if (myRun === run) {
      const ended = endedBy(err);
      if (ended) end(ended);
      else setFlowError(loginErrorText(err));
    }
    throw err;
  } finally {
    setFlowBusy(false);
  }
}

/** "Try again" after an ended attempt: a new QR, or back to the form. */
export async function retryLoginFlow(): Promise<void> {
  const f = flow();
  if (f.step === "ended" && f.via === "qr") {
    await startPhoneSignIn(serverUrl);
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
