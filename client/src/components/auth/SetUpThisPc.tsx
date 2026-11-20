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
  pollPcSetup,
  startPcSetup,
  startPcSetupPasskey,
  type PcSetupStatus,
} from "@/lib/tauri/pcSetup";
import { replaceSessionWithPasskey } from "@/stores/auth";
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

type Patch = Pick<PcSetupStatus, "state"> & Partial<Pick<PcSetupStatus, "reason" | "enrollment_id">>;

/**
 * What a platform step gets: the attempt, and a way to report what it learned
 * about that attempt (the dialog drops a report about an earlier attempt).
 */
export interface PcSetupStepProps {
  attemptId: string;
  onState: (attemptId: string, patch: Patch) => void;
}

/** The steps that differ between the web page and the desktop app (Task 6). */
export interface PcSetupPlatform {
  /** State `code`: the person types the phone's code. */
  CodeStep: Component<PcSetupStepProps>;
  /** State `enrolled`: the passkey is created. */
  PasskeyStep: Component<PcSetupStepProps>;
}

// ---------------------------------------------------------------------------
// Web steps
// ---------------------------------------------------------------------------

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
    setBusy(true);
    setError(null);
    try {
      const res = await enterPcSetupCode(id, code());
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
  let ctl: AbortController | null = null;
  onCleanup(() => ctl?.abort());

  const register = async () => {
    const id = props.attemptId;
    ctl?.abort();
    const mine = new AbortController();
    ctl = mine;
    setBusy(true);
    setError(null);
    try {
      const start = await startPcSetupPasskey(id);
      const credential = await createPasskeyCredential(start.options, mine.signal);
      await finishPcSetupPasskey(id, start.ceremony_id, credential, defaultPasskeyLabel());
      if (!mine.signal.aborted) props.onState(id, { state: "completed", reason: null });
    } catch (err) {
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

  return (
    <div class="space-y-3">
      <p>{t("pcSetup.registerIntro")}</p>
      <Show when={busy()}>
        <p role="status" class="text-sm text-text-secondary">{t("pcSetup.registering")}</p>
      </Show>
      <Show when={error()}>
        <p role="alert" class="text-sm text-text-primary">{error()}</p>
        <LinuxPasskeyHint class="text-sm text-text-secondary" />
      </Show>
      <button type="button" class="btn-primary min-h-11 text-base" disabled={busy()} onClick={() => void register()}>
        {t("pcSetup.register")}
      </button>
    </div>
  );
};

const WEB_PLATFORM: PcSetupPlatform = { CodeStep: WebCodeStep, PasskeyStep: WebPasskeyStep };

// ---------------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------------

const isLive = (s: PcSetupStatus | null): boolean => !!s && LIVE_PC_SETUP_STATES.includes(s.state);

/** SM5 order of the live states and `completed`; every other state is an end. */
const RANK: Record<string, number> = { waiting: 0, claimed: 1, code: 2, enrolled: 3, completed: 4 };
const rank = (state: string): number => RANK[state] ?? 5;

/** States the client knows (C27); anything else reads as the generic text. */
const KNOWN_ENDS = ["completed", "refused", "reported", "failed", "cancelled", "expired"];

/** The sentence for an ended attempt (the PC error table of the spec). */
function endedKey(s: PcSetupStatus): MessageKey {
  if (!LIVE_PC_SETUP_STATES.includes(s.state) && !KNOWN_ENDS.includes(s.state)) return "pcSetup.ended.generic";
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
  /** Bumped by every start, restart and the unmount: older loops and answers stop. */
  let generation = 0;
  let signIn: AbortController | null = null;
  /** The dialog is gone: a start answered later is given up at once. */
  let closed = false;
  /** The open long-poll: ended by a restart and by the unmount (it holds a server slot). */
  let polling: AbortController | null = null;

  const apply = (res: PcSetupStatus) => {
    const cur = status();
    // Only answers about the attempt on screen count.
    if (cur && cur.attempt_id !== res.attempt_id) return;
    // A late answer never takes a step back (a code just typed, a finished
    // passkey); only `claimed` may return to `waiting` (released, or expired).
    if (cur && rank(res.state) < rank(cur.state) && !(cur.state === "claimed" && res.state === "waiting")) return;
    setStatus((prev) => ({ ...res, enrollment_id: res.enrollment_id ?? prev?.enrollment_id }));
    if (res.state !== "waiting") setQr("");
    else if (res.qr_payload) setQr(res.qr_payload);
  };

  const patch = (attemptId: string, p: Patch) => {
    const cur = status();
    if (cur?.attempt_id !== attemptId) return;
    // As for polls: never leave an end, never take a step back.
    if (rank(cur.state) >= 5 || rank(p.state) < rank(cur.state)) return;
    setStatus((prev) => (prev ? { ...prev, reason: null, ...p } : prev));
    if (p.state !== "waiting") setQr("");
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
        if (mine !== generation) return;
        if (err instanceof HttpError && err.status === 404) {
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

  const begin = async () => {
    const mine = ++generation;
    polling?.abort();
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
      setStatus({ attempt_id: started.attempt_id, state: "waiting", reason: null });
      setQr(started.qr_payload);
      setStarting(false);
      void pollLoop(mine);
    } catch (err) {
      if (mine !== generation) return;
      setStarting(false);
      setStartError(
        errorText(err, "pcSetup.startFailed", { PC_SETUP_UNAVAILABLE: "pcSetup.unavailable" }),
      );
    }
  };

  /** Pick up the session's live attempt (a reload), else start a new one. */
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
      apply(res);
      setStarting(false);
      void pollLoop(mine);
    } catch (err) {
      if (mine !== generation) return;
      if (err instanceof HttpError && err.status === 404) void begin();
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
    // Closing the dialog gives a live attempt up.
    const s = status();
    if (s && isLive(s)) void cancelPcSetup(s.attempt_id).catch(() => undefined);
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

  return (
    <DialogShell title={t("pcSetup.title")} onClose={() => props.onClose()} wide testId="set-up-this-pc">
      <Show when={starting()}>
        <p role="status" class="text-text-secondary">{t("pcSetup.starting")}</p>
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
              <Dynamic component={platform().CodeStep} attemptId={s().attempt_id} onState={patch} />
            </Match>
            <Match when={s().state === "enrolled"}>
              <Dynamic component={platform().PasskeyStep} attemptId={s().attempt_id} onState={patch} />
            </Match>
            <Match when={s().state === "completed"}>
              <div class="space-y-3">
                <p>{t("pcSetup.completed")}</p>
                <button
                  type="button"
                  class="btn-primary min-h-11 text-base"
                  disabled={signingIn()}
                  onClick={() => void signInWithPasskey()}
                >
                  {t("pcSetup.signIn")}
                </button>
                <Show when={signInError()}>
                  <p role="alert" class="text-sm text-text-primary">{signInError()}</p>
                  <LinuxPasskeyHint class="text-sm text-text-secondary" />
                </Show>
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
          disabled={starting()}
          onClick={() => void begin()}
        >
          {t("pcSetup.startAgain")}
        </button>
      </div>
    </DialogShell>
  );
};

export default SetUpThisPc;
