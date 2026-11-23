/**
 * SetUpThisPc — "Set up this PC" (SM5), the PC's side. A chat-only session
 * asks, the Kaiku app on the person's phone scans the QR code and grants, the
 * person types the 6-digit code the phone shows, a passkey is created, and
 * the person signs in with it (a new, full session).
 *
 * The attempt lives on the server: the dialog resumes it after a reload
 * (`currentPcSetup`), polls it (a long-poll, never closer than once a second)
 * and offers "Start again" in every state. The two steps that differ between
 * the web page and the desktop app, typing the code and creating the passkey,
 * are the `platform` seam: the web steps are the default, the desktop app
 * passes its own (the system browser takes the code and the ceremony there).
 */

import { Component, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { Dynamic } from "solid-js/web";
import { plural, t, type MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { codeIs } from "@/lib/errorCode";
import { PASSKEY_ERRORS, passkeyFailureText } from "@/lib/authErrors";
import { HttpError, isTauri } from "@/lib/tauri/common";
import { createPasskeyCredential } from "@/lib/tauri/passkeys";
import { defaultPasskeyLabel } from "@/lib/passkeyLabel";
import {
  LIVE_PC_SETUP_STATES,
  cancelPcSetup,
  currentPcSetup,
  enterPcSetupCode,
  finishPcSetupPasskey,
  newPcSetupProof,
  pollPcSetup,
  startPcSetup,
  startPcSetupPasskey,
  type PcSetupStatus,
} from "@/lib/tauri/pcSetup";
import { replaceSessionWithPasskey, sessionInfo } from "@/stores/auth";
import DialogShell from "./DialogShell";
import QrCodeImage from "./QrCodeImage";
import CodeEntry from "./CodeEntry";
import LinuxPasskeyHint from "./LinuxPasskeyHint";
import { DESKTOP_PLATFORM } from "./SetUpThisPcDesktop";

/** Never poll closer than this, however fast the server answers. */
export const MIN_POLL_GAP_MS = 1_000;
/** The long-poll's wait (the server allows up to 25 s). */
const POLL_WAIT_S = 20;
/** First pause after a failed poll; it doubles up to [`POLL_RETRY_MAX_MS`]. */
const POLL_RETRY_MS = 3_000;
const POLL_RETRY_MAX_MS = 30_000;
/** Reads of a replaced attempt's final state after a restart, and the first gap. */
const FINISH_READS = 3;
const FINISH_READ_GAP_MS = 1_000;

type Patch = Pick<PcSetupStatus, "state"> & Partial<Pick<PcSetupStatus, "reason" | "enrollment_id">>;

/**
 * What a platform step gets: the attempt, and a way to report what it learned
 * about that attempt (the dialog drops a report about an earlier attempt).
 */
export interface PcSetupStepProps {
  attemptId: string;
  onState: (attemptId: string, patch: Patch) => void;
  /**
   * The proofs this dialog sent with a code on the web, by attempt id (in
   * memory only: a reload loses them, and the passkey then needs Start again).
   */
  proofs: Map<string, string>;
  /**
   * Attempts whose proof this page may have had stored by the server: a code
   * request that carried it was answered "right", or its answer is unknown.
   * Kept per session for the page's lifetime, apart from the proof's value.
   */
  accepted: Set<string>;
  /**
   * What became of this dialog's own registration of an attempt (web only):
   * a finish request was sent, or a sent finish was refused beyond doubt.
   */
  onFinish: (attemptId: string, phase: FinishPhase) => void;
}

/** A finish request of this dialog: sent, or refused beyond doubt. */
export type FinishPhase = "sent" | "refused";

/** The steps that differ between the web page and the desktop app (Task 6). */
export interface PcSetupPlatform {
  /** State `code`: the person types the phone's code. */
  CodeStep: Component<PcSetupStepProps>;
  /** State `enrolled`: the passkey is created. */
  PasskeyStep: Component<PcSetupStepProps>;
  /**
   * Whether this dialog registers the passkey itself (the web page). Then
   * `completed` is success only for a registration this dialog finished.
   */
  registersHere: boolean;
}

// ---------------------------------------------------------------------------
// Web steps
// ---------------------------------------------------------------------------

/**
 * An answer that refuses a request beyond doubt: a 4xx other than 408 and
 * 429. A lost answer, a timeout, a 5xx, a 408 or a 429 leave open whether the
 * server acted on it.
 */
const refusedOutright = (err: unknown): boolean =>
  err instanceof HttpError && err.status >= 400 && err.status < 500 && err.status !== 408 && err.status !== 429;

/** The keypad: the code is typed in this page. */
export const WebCodeStep: Component<PcSetupStepProps> = (props) => {
  const [code, setCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  /** The step is gone: a late answer changes nothing. */
  let gone = false;
  onCleanup(() => (gone = true));

  const submit = async () => {
    const id = props.attemptId;
    // One proof per attempt: a retyped code (or a code sent again after a
    // lost answer) carries the same one.
    const proofs = props.proofs;
    const accepted = props.accepted;
    let proof = proofs.get(id);
    if (!proof) {
      proof = newPcSetupProof();
      proofs.set(id, proof);
    }
    setBusy(true);
    setError(null);
    try {
      const res = await enterPcSetupCode(id, code(), proof).catch((err: unknown) => {
        // An unknown answer may have stored the proof; a refusal beyond doubt
        // changes nothing.
        if (!refusedOutright(err)) accepted.add(id);
        throw err;
      });
      // "Right" stores the proof; "wrong" shows that no request with it was
      // right (a right one would have moved the attempt on).
      if (res.state === "enrolled") accepted.add(id);
      else if (res.state === "code") accepted.delete(id);
      if (gone) return;
      if (res.state === "enrolled") props.onState(id, { state: "enrolled", reason: null, enrollment_id: res.enrollment_id });
      else if (res.state === "failed") props.onState(id, { state: "failed", reason: "too_many_codes" });
      else
        setError(
          typeof res.tries_left === "number"
            ? plural(res.tries_left, { one: "pcSetup.codeWrong.one", other: "pcSetup.codeWrong.other" })
            : t("pcSetup.codeFailed"),
        );
    } catch (err) {
      if (gone) return;
      if (err instanceof HttpError && codeIs(err.code, "APPROVAL_EXPIRED")) {
        props.onState(id, { state: "expired", reason: "code_expired" });
      } else {
        // A state conflict is corrected by the next poll.
        setError(errorText(err, "pcSetup.codeFailed"));
      }
    } finally {
      if (!gone) {
        setCode("");
        setBusy(false);
      }
    }
  };

  return (
    <div class="space-y-3">
      <p>{t("pcSetup.codeIntro")}</p>
      <p class="text-sm text-text-primary">{t("pcSetup.codeOwnPhoneOnly")}</p>
      <CodeEntry
        length={6}
        value={code()}
        onInput={setCode}
        onSubmit={() => void submit()}
        disabled={busy()}
        label={t("pcSetup.codeLabel")}
      />
      <Show when={error()}>
        <p role="alert" class="text-sm text-text-primary">{error()}</p>
      </Show>
    </div>
  );
};

/** The passkey is created in this browser, on the user's click. */
export const WebPasskeyStep: Component<PcSetupStepProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  /** The finish was answered, but not with the stored passkey: the poll decides. */
  const [unclear, setUnclear] = createSignal(false);
  let ctl: AbortController | null = null;
  onCleanup(() => ctl?.abort());

  const register = async () => {
    const id = props.attemptId;
    ctl?.abort();
    const mine = new AbortController();
    ctl = mine;
    setBusy(true);
    setError(null);
    setUnclear(false);
    let finishSent = false;
    try {
      // Only the proof sent with the right code starts it (409 otherwise).
      const start = await startPcSetupPasskey(id, props.proofs.get(id));
      const credential = await createPasskeyCredential(start.options, mine.signal);
      const finish = finishPcSetupPasskey(id, start.ceremony_id, credential, defaultPasskeyLabel(), props.proofs.get(id));
      finishSent = true;
      props.onFinish(id, "sent");
      const stored = await finish;
      // Only the server's own answer (a 201 with the passkey's id) says the
      // passkey exists; any other success leaves it to the poll.
      if (!stored || typeof stored.id !== "string") {
        setUnclear(true);
        return;
      }
      // The passkey exists: say so even if the step was left meanwhile (the
      // dialog drops it for another attempt or after a server-reported end).
      props.onState(id, { state: "completed", reason: null });
    } catch (err) {
      // Only a finish the server refused beyond doubt means this dialog did
      // not register a passkey; an error of the start never changes that.
      if (finishSent && refusedOutright(err)) props.onFinish(id, "refused");
      if (!mine.signal.aborted) {
        // The attempt cannot take a passkey this way: only Start again helps.
        if (err instanceof HttpError && codeIs(err.code, "APPROVAL_STATE")) setError(t("pcSetup.ended.generic"));
        else setError(passkeyFailureText(err, "pcSetup.registerFailed", PASSKEY_ERRORS) || null);
      }
    } finally {
      if (ctl === mine) {
        ctl = null;
        setBusy(false);
      }
    }
  };

  // Only the page that typed the code holds its proof and can register; any
  // other page of the session (another tab, a reload) says where it happens.
  return (
    <Show
      when={props.proofs.has(props.attemptId) && props.accepted.has(props.attemptId)}
      fallback={<p role="status">{t("pcSetup.otherPageFinishing")}</p>}
    >
      <div class="space-y-3">
        <p>{t("pcSetup.registerIntro")}</p>
        <LinuxPasskeyHint class="text-sm text-text-secondary" />
        <Show when={busy()}>
          <p role="status" class="text-sm text-text-secondary">{t("pcSetup.registering")}</p>
        </Show>
        <Show when={unclear()}>
          <p role="status" class="text-sm text-text-secondary">{t("pcSetup.finishUnclear")}</p>
        </Show>
        <Show when={error()}>
          <p role="alert" class="text-sm text-text-primary">{error()}</p>
        </Show>
        <button type="button" class="btn-primary min-h-11 text-base" disabled={busy()} onClick={() => void register()}>
          {t("pcSetup.register")}
        </button>
      </div>
    </Show>
  );
};

const WEB_PLATFORM: PcSetupPlatform = { CodeStep: WebCodeStep, PasskeyStep: WebPasskeyStep, registersHere: true };

// ---------------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------------

const isLive = (s: PcSetupStatus | null): boolean => !!s && LIVE_PC_SETUP_STATES.includes(s.state);

/** SM5 order of the live states and `completed`; every other state is an end. */
const RANK: Record<string, number> = { waiting: 0, claimed: 1, code: 2, enrolled: 3, completed: 4 };
const rank = (state: string): number => RANK[state] ?? 5;

/** States the client knows (C27); anything else reads as the generic text. */
const KNOWN_ENDS = ["completed", "refused", "reported", "failed", "cancelled", "expired"];
const KNOWN_STATES: readonly string[] = [...LIVE_PC_SETUP_STATES, ...KNOWN_ENDS];

/**
 * What the page knows about its own attempts, by session id: in memory for
 * the page's lifetime (never in storage), so it outlives the dialog but not
 * a reload.
 */
interface PageMemory {
  /** A replaced or left attempt whose end is not known yet: read before a new or resumed code. */
  unresolved: string | null;
  /** Per attempt: finish requests sent, and how many the server refused beyond doubt. */
  finishes: Map<string, { sent: number; refused: number }>;
  /** Attempts whose proof the server may have stored (see `PcSetupStepProps.accepted`). */
  accepted: Set<string>;
}
const pageMemory = new Map<string, PageMemory>();
function memoryOf(sessionKey: string): PageMemory {
  let m = pageMemory.get(sessionKey);
  if (!m) {
    m = { unresolved: null, finishes: new Map(), accepted: new Set() };
    pageMemory.set(sessionKey, m);
  }
  return m;
}

/** Reasons the client knows (C27, plus its own `gone` and `poll_refused`). */
const KNOWN_REASONS = [
  "session_ended",
  "device_revoked",
  "no_key_device",
  "may_approve_off",
  "account_banned",
  "timeout",
  "code_expired",
  "restarted",
  "cancelled",
  "refused",
  "reported",
  "too_many_codes",
  "enrollment_expired",
  "gone",
  "poll_refused",
];

/**
 * An end the client made up (a poll refused for a reason the server did not
 * report): a server-confirmed end may still replace it.
 */
const clientMade = (s: PcSetupStatus): boolean => s.reason === "poll_refused";

/** An unknown state or reason: the generic failure, never a specific end. */
const isUnknown = (s: Pick<PcSetupStatus, "state" | "reason">): boolean =>
  (!LIVE_PC_SETUP_STATES.includes(s.state) && !KNOWN_ENDS.includes(s.state)) ||
  (s.reason != null && !KNOWN_REASONS.includes(s.reason));

/** The sentence for an ended attempt (the PC error table of the spec). */
function endedKey(s: PcSetupStatus): MessageKey {
  if (isUnknown(s)) return "pcSetup.ended.generic";
  if (s.state === "refused") return "pcSetup.ended.refused";
  if (s.state === "reported") return "pcSetup.ended.reported";
  if (s.state === "failed") return "pcSetup.ended.tooManyCodes";
  switch (s.reason) {
    case "may_approve_off":
    case "device_revoked":
    case "no_key_device":
      return "pcSetup.ended.mayApproveOff";
    case "session_ended":
      return "pcSetup.ended.sessionEnded";
    case "account_banned":
      return "auth.login.banned";
    case "refused":
      return "pcSetup.ended.refused";
    case "reported":
      return "pcSetup.ended.reported";
    case "too_many_codes":
      return "pcSetup.ended.tooManyCodes";
    case "restarted":
    case "cancelled":
      return "pcSetup.ended.cancelled";
    case "gone":
      return "pcSetup.ended.gone";
    case "timeout":
    case "code_expired":
    case "enrollment_expired":
      return "pcSetup.ended.timeout";
    case null:
    case undefined:
      return s.state === "cancelled" ? "pcSetup.ended.cancelled" : "pcSetup.ended.generic";
    default:
      return "pcSetup.ended.generic";
  }
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export interface SetUpThisPcProps {
  onClose: () => void;
  platform?: PcSetupPlatform;
}

const SetUpThisPc: Component<SetUpThisPcProps> = (props) => {
  const platform = () => props.platform ?? (isTauri() ? DESKTOP_PLATFORM : WEB_PLATFORM);
  const [status, setStatus] = createSignal<PcSetupStatus | null>(null);
  const [qr, setQr] = createSignal("");
  const [starting, setStarting] = createSignal(true);
  const [startError, setStartError] = createSignal<string | null>(null);
  const [offline, setOffline] = createSignal(false);
  const [signingIn, setSigningIn] = createSignal(false);
  const [signInError, setSignInError] = createSignal<string | null>(null);
  /** Reading the replaced attempt's end after a restart. */
  const [checking, setChecking] = createSignal(false);
  const [checkFailed, setCheckFailed] = createSignal(false);
  /** The proofs sent with a code from this dialog, by attempt id (the values). */
  const proofs = new Map<string, string>();
  /** The session this dialog belongs to, and what the page knows about its attempts. */
  const sessionKey = sessionInfo()?.id ?? "";
  const memory = memoryOf(sessionKey);
  /** Bumped when the page's record of its finishes changes (it is not reactive itself). */
  const [finishTick, setFinishTick] = createSignal(0);
  const onFinish = (attemptId: string, phase: FinishPhase) => {
    const cur = memory.finishes.get(attemptId) ?? { sent: 0, refused: 0 };
    memory.finishes.set(
      attemptId,
      phase === "sent" ? { ...cur, sent: cur.sent + 1 } : { ...cur, refused: cur.refused + 1 },
    );
    setFinishTick((n) => n + 1);
  };
  /**
   * `completed` of this attempt is this page's passkey (always so on the
   * desktop): a finish it sent was not refused beyond doubt, whatever reason
   * the status carries.
   */
  const registeredHere = (attemptId: string): boolean => {
    finishTick();
    if (!platform().registersHere) return true;
    const f = memory.finishes.get(attemptId);
    return !!f && f.sent > f.refused;
  };
  /** Bumped by every start, restart and the unmount: older loops and answers stop. */
  let generation = 0;
  let signIn: AbortController | null = null;
  /** The dialog is gone: a start answered later is given up at once. */
  let closed = false;
  /** The open long-poll: ended by a restart and by the unmount (it holds a server slot). */
  let polling: AbortController | null = null;

  /** Entering a state that is not live ends the polling at once. */
  const ended = (state: PcSetupStatus["state"]) => {
    if (!LIVE_PC_SETUP_STATES.includes(state)) polling?.abort();
  };

  const apply = (input: PcSetupStatus) => {
    const cur = status();
    // Only answers about the attempt on screen count.
    if (cur && cur.attempt_id !== input.attempt_id) return;
    // An unknown reason, also on a live state, is the generic end (C27).
    const res: PcSetupStatus = isUnknown(input) && isLive(input) ? { ...input, state: "expired" } : input;
    if (cur && !isLive(cur)) {
      // Every server-reported end (`completed` too) is final; an end the
      // client made up gives way to an end the server reports.
      if (!clientMade(cur) || isLive(res) || clientMade(res)) return;
    } else if (cur && rank(res.state) < rank(cur.state) && !(cur.state === "claimed" && res.state === "waiting")) {
      // A late answer never takes a step back (a code just typed, a finished
      // passkey); only `claimed` may return to `waiting` (released, or expired).
      return;
    }
    setStatus((prev) => ({ ...res, enrollment_id: res.enrollment_id ?? prev?.enrollment_id }));
    if (res.state !== "waiting") setQr("");
    else if (res.qr_payload) setQr(res.qr_payload);
    ended(res.state);
  };

  const patch = (attemptId: string, p: Patch) => {
    const cur = status();
    if (cur?.attempt_id !== attemptId) return;
    // As for polls: never leave a server-reported end, never take a step back.
    if (!isLive(cur)) {
      if (!clientMade(cur) || LIVE_PC_SETUP_STATES.includes(p.state)) return;
    } else if (rank(p.state) < rank(cur.state)) return;
    setStatus((prev) => (prev ? { ...prev, reason: null, ...p } : prev));
    if (p.state !== "waiting") setQr("");
    ended(p.state);
  };

  const pollLoop = async (mine: number) => {
    let retry = POLL_RETRY_MS;
    while (mine === generation) {
      const current = status();
      if (!current || !isLive(current)) return;
      const began = Date.now();
      let pause = 0;
      const ctl = new AbortController();
      polling = ctl;
      try {
        const res = await pollPcSetup(current.attempt_id, POLL_WAIT_S, ctl.signal);
        if (mine !== generation) return;
        setOffline(false);
        retry = POLL_RETRY_MS;
        apply(res);
      } catch (err) {
        if (mine !== generation || ctl.signal.aborted) return;
        // Only the server's own "not found" ends the attempt as gone; any
        // other 404 is an answer from somewhere else (a client-made end below).
        if (err instanceof HttpError && err.status === 404 && codeIs(err.code, "APPROVAL_NOT_FOUND")) {
          apply({ attempt_id: current.attempt_id, state: "expired", reason: "gone" });
          return;
        }
        // A refusal that waiting does not change: end with the generic text.
        // (401: a refresh that failed for now; a real session end signs out
        // and closes the dialog on its own.)
        if (
          err instanceof HttpError &&
          err.status >= 400 &&
          err.status < 500 &&
          ![401, 408, 429].includes(err.status)
        ) {
          apply({ attempt_id: current.attempt_id, state: "expired", reason: "poll_refused" });
          return;
        }
        setOffline(true);
        const asked = err instanceof HttpError && err.retryAfter ? err.retryAfter * 1_000 : 0;
        pause = Math.max(retry, asked);
        retry = Math.min(retry * 2, POLL_RETRY_MAX_MS);
      } finally {
        if (polling === ctl) polling = null;
      }
      pause = Math.max(pause, MIN_POLL_GAP_MS - (Date.now() - began));
      if (pause > 0) await sleep(pause);
    }
  };

  /**
   * The attempt a restart created (or the one resumed after it), held back
   * until the replaced attempt's end is read.
   */
  let pending: {
    mine: number;
    oldId: string;
    started: { attempt_id: string; qr_payload: string };
    /** A resumed attempt (any live state), shown as it is after the read. */
    resumed?: PcSetupStatus;
  } | null = null;
  const unresolved = (): string | null => memory.unresolved;
  const setUnresolved = (id: string | null) => {
    memory.unresolved = id;
  };

  /** One read of attempt `id`: its status if the answer is about it in a known state, else null. */
  const readOld = async (id: string): Promise<PcSetupStatus | null> => {
    // The contract's "no such attempt" is a known end: it no longer exists.
    const read = await pollPcSetup(id, 0).catch((err: unknown) =>
      err instanceof HttpError && err.status === 404 && codeIs(err.code, "APPROVAL_NOT_FOUND")
        ? ({ attempt_id: id, state: "expired", reason: "gone" } satisfies PcSetupStatus)
        : null,
    );
    return read && read.attempt_id === id && KNOWN_STATES.includes(read.state) ? read : null;
  };

  const showStarted = (mine: number, started: { attempt_id: string; qr_payload: string }) => {
    setStatus({ attempt_id: started.attempt_id, state: "waiting", reason: null });
    setQr(started.qr_payload);
    setStarting(false);
    void pollLoop(mine);
  };

  /**
   * After a restart from a shown attempt whose end the server has not
   * reported (any live state, or an end the client made up), the replaced
   * attempt's end is read before the new code is shown. The server decided
   * "finish or restart" under one lock before the start answered, so this
   * read is final: `completed` is shown (and the new attempt given up),
   * anything else shows the new code. Only an answer with that attempt's id
   * and a state the client knows counts; three failed reads offer a retry,
   * never the code.
   */
  const resolvePrevious = async () => {
    const p = pending;
    if (!p) return;
    setCheckFailed(false);
    setChecking(true);
    let old: PcSetupStatus | null = null;
    for (let i = 0; i < FINISH_READS; i++) {
      old = await readOld(p.oldId);
      if (pending !== p || p.mine !== generation) return;
      if (old) break;
      if (i + 1 < FINISH_READS) await sleep(FINISH_READ_GAP_MS * 2 ** i);
      if (pending !== p || p.mine !== generation) return;
    }
    if (!old) {
      setCheckFailed(true);
      return;
    }
    pending = null;
    if (unresolved() === p.oldId) setUnresolved(null);
    setChecking(false);
    if (old.state === "completed") {
      void cancelPcSetup(p.started.attempt_id).catch(() => undefined);
      setStarting(false);
      setStatus({ ...old, attempt_id: p.oldId });
      return;
    }
    if (p.resumed) {
      apply(p.resumed);
      setStarting(false);
      void pollLoop(p.mine);
      return;
    }
    showStarted(p.mine, p.started);
  };

  const begin = async () => {
    const mine = ++generation;
    polling?.abort();
    // Locking 6: a finish may have won against this restart. The attempt
    // whose end is not known yet (a check still open counts too, and one
    // left by an earlier dialog of this session) is read before the new
    // code is shown, whatever state this dialog last saw.
    const previous = status();
    const unknownEnd =
      pending?.oldId ??
      unresolved() ??
      (previous && (isLive(previous) || clientMade(previous)) ? previous.attempt_id : null);
    // Until it is read, it outlives this dialog.
    if (unknownEnd) setUnresolved(unknownEnd);
    if (pending) void cancelPcSetup(pending.started.attempt_id).catch(() => undefined);
    pending = null;
    setChecking(false);
    setCheckFailed(false);
    setStatus(null);
    setQr("");
    setStartError(null);
    setOffline(false);
    setStarting(true);
    try {
      const started = await startPcSetup();
      if (mine !== generation) {
        // The dialog closed meanwhile: give up the attempt the server just made.
        if (closed) void cancelPcSetup(started.attempt_id).catch(() => undefined);
        return;
      }
      if (unknownEnd) {
        pending = { mine, oldId: unknownEnd, started };
        await resolvePrevious();
        return;
      }
      showStarted(mine, started);
    } catch (err) {
      if (mine !== generation) return;
      // The attempt this start wanted to replace is read first: an end it
      // already has (a passkey that exists above all) is shown instead of the
      // start's failure. Otherwise its end stays unknown for a later start.
      if (unknownEnd) {
        const old = await readOld(unknownEnd);
        if (mine !== generation) return;
        if (old && !isLive(old)) {
          if (unresolved() === unknownEnd) setUnresolved(null);
          setStarting(false);
          setStatus(old);
          return;
        }
      }
      setStarting(false);
      setStartError(
        errorText(err, "pcSetup.startFailed", { PC_SETUP_UNAVAILABLE: "pcSetup.unavailable" }),
      );
    }
  };

  /**
   * Pick up the session's live attempt (a reload, a reopened dialog), else
   * start a new one, but only on the server's own "no attempt": any other
   * answer shows the start error with Start again and cancels nothing.
   */
  const resumeOrBegin = async () => {
    const mine = ++generation;
    polling?.abort();
    try {
      const res = await currentPcSetup();
      if (mine !== generation) {
        // The dialog closed meanwhile: give the resumed attempt up.
        if (closed && isLive(res)) void cancelPcSetup(res.attempt_id).catch(() => undefined);
        return;
      }
      const old = unresolved();
      if (old === res.attempt_id) setUnresolved(null);
      else if (old && isLive(res)) {
        // A replacement, in whatever live state: the replaced attempt's end
        // is read before it is shown.
        pending = {
          mine,
          oldId: old,
          started: { attempt_id: res.attempt_id, qr_payload: res.qr_payload ?? "" },
          resumed: res,
        };
        await resolvePrevious();
        return;
      }
      apply(res);
      setStarting(false);
      void pollLoop(mine);
    } catch (err) {
      if (mine !== generation) return;
      if (err instanceof HttpError && err.status === 404 && codeIs(err.code, "APPROVAL_NOT_FOUND")) void begin();
      else {
        setStarting(false);
        setStartError(errorText(err, "pcSetup.startFailed"));
      }
    }
  };

  onMount(() => void resumeOrBegin());

  onCleanup(() => {
    closed = true;
    generation++;
    polling?.abort();
    signIn?.abort();
    // Closing the dialog gives a live attempt up, and one a restart made;
    // the replaced attempt's unknown end stays for the next dialog.
    if (pending) {
      setUnresolved(pending.oldId);
      void cancelPcSetup(pending.started.attempt_id).catch(() => undefined);
    }
    pending = null;
    const s = status();
    if (s && isLive(s)) {
      // A finish this page sent may still have stored a passkey: that end is
      // read before any new code.
      if ((memory.finishes.get(s.attempt_id)?.sent ?? 0) > 0) setUnresolved(s.attempt_id);
      void cancelPcSetup(s.attempt_id).catch(() => undefined);
    }
  });

  const signInWithPasskey = async () => {
    signIn?.abort();
    const ctl = new AbortController();
    signIn = ctl;
    setSigningIn(true);
    setSignInError(null);
    try {
      await replaceSessionWithPasskey(ctl.signal);
      props.onClose();
    } catch (err) {
      if (!ctl.signal.aborted) {
        setSignInError(passkeyFailureText(err, "auth.probation.signInFailed", PASSKEY_ERRORS) || null);
      }
    } finally {
      if (signIn === ctl) {
        signIn = null;
        setSigningIn(false);
      }
    }
  };

  /** "Sign in with your passkey" and its error; `primary` on the success screen. */
  const signInButton = (primary: boolean) => (
    <>
      <button
        type="button"
        class={
          primary
            ? "btn-primary min-h-11 text-base"
            : "min-h-11 px-3 rounded-lg text-base text-text-primary bg-surface-layer2 hover:bg-white/10"
        }
        disabled={signingIn()}
        onClick={() => void signInWithPasskey()}
      >
        {t("pcSetup.signIn")}
      </button>
      <Show when={signInError()}>
        <p role="alert" class="text-sm text-text-primary">{signInError()}</p>
        <LinuxPasskeyHint class="text-sm text-text-secondary" />
      </Show>
    </>
  );

  return (
    <DialogShell title={t("pcSetup.title")} onClose={() => props.onClose()} wide testId="set-up-this-pc">
      <Show when={starting() && !checking()}>
        <p role="status" class="text-text-secondary">{t("pcSetup.starting")}</p>
      </Show>
      <Show when={checking()}>
        <Show
          when={checkFailed()}
          fallback={<p role="status" class="text-text-secondary">{t("pcSetup.checkingPrevious")}</p>}
        >
          <div class="space-y-3">
            <p role="alert" class="text-text-primary">{t("pcSetup.checkFailed")}</p>
            <button type="button" class="btn-primary min-h-11 text-base" onClick={() => void resolvePrevious()}>
              {t("common.tryAgain")}
            </button>
          </div>
        </Show>
      </Show>
      <Show when={startError()}>
        <p role="alert" class="text-text-primary">{startError()}</p>
      </Show>
      <Show when={status()}>
        {(s) => (
          <Switch>
            <Match when={s().state === "waiting"}>
              <div class="space-y-3 text-center">
                <p class="text-left">{t("pcSetup.waiting")}</p>
                <Show when={qr()} fallback={<p role="status" class="text-text-secondary">{t("pcSetup.starting")}</p>}>
                  <QrCodeImage payload={qr()} alt={t("pcSetup.qrAlt")} />
                </Show>
              </div>
            </Match>
            <Match when={s().state === "claimed"}>
              <p role="status">{t("pcSetup.claimed")}</p>
            </Match>
            <Match when={s().state === "code"}>
              <Dynamic
                component={platform().CodeStep}
                attemptId={s().attempt_id}
                onState={patch}
                proofs={proofs}
                accepted={memory.accepted}
                onFinish={onFinish}
              />
            </Match>
            <Match when={s().state === "enrolled"}>
              <Dynamic
                component={platform().PasskeyStep}
                attemptId={s().attempt_id}
                onState={patch}
                proofs={proofs}
                accepted={memory.accepted}
                onFinish={onFinish}
              />
            </Match>
            <Match when={s().state === "completed" && !registeredHere(s().attempt_id)}>
              <div class="space-y-3">
                <p role="status">{t("pcSetup.ended.otherPage")}</p>
                {/* If it was the person, the passkey signs in here too (secondary). */}
                {signInButton(false)}
              </div>
            </Match>
            <Match when={s().state === "completed"}>
              <div class="space-y-3">
                <p>{t("pcSetup.completed")}</p>
                {signInButton(true)}
              </div>
            </Match>
            <Match when={true}>
              <p role="alert">{t(endedKey(s()))}</p>
            </Match>
          </Switch>
        )}
      </Show>
      <Show when={offline()}>
        <p role="status" class="text-sm text-text-secondary">{t("pcSetup.pollFailed")}</p>
      </Show>
      <div class="flex justify-end">
        <button
          type="button"
          class="min-h-11 px-3 rounded-lg text-base text-text-primary hover:bg-white/10 state-disabled"
          disabled={starting() && !checkFailed()}
          onClick={() => void begin()}
        >
          {t("pcSetup.startAgain")}
        </button>
      </div>
    </DialogShell>
  );
};

export default SetUpThisPc;
