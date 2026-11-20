/**
 * The desktop steps of "Set up this PC" (SM5): the code is not typed in the
 * app. The app starts the server's `pc_setup` flow (Rust: `passkey_pc_setup`),
 * the system browser asks for the phone's code and creates the passkey, and
 * the app redeems the result, which carries no session.
 *
 * The flow outlives the step components: it begins in state `code` and runs on
 * through `enrolled`, so it is held per attempt here and ended only when no
 * step shows it any more. After an app restart the verifier is gone with the
 * old process: a `code` attempt can open a new flow, an `enrolled` one needs
 * "Start again" (the dialog's button).
 */

import { Component, Show, createSignal, onCleanup } from "solid-js";
import { t } from "@/i18n";
import { PASSKEY_ERRORS, passkeyFailureText } from "@/lib/authErrors";
import { PasskeyError, setUpThisPcInBrowser } from "@/lib/tauri/passkeys";
import type { PcSetupPlatform, PcSetupStepProps } from "./SetUpThisPc";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

interface BrowserFlow {
  ctl: AbortController;
  error: () => string | null;
  /** The failure has its own passkey text (limit, already added, …). */
  passkeyText: () => boolean;
  /** The command has returned (nothing left to cancel). */
  done: boolean;
}

const flows = new Map<string, BrowserFlow>();
/** Steps showing each attempt's flow. */
const refs = new Map<string, number>();
const [version, setVersion] = createSignal(0);

/**
 * The text of a failed browser step: the passkey texts for passkey errors
 * (limit, already added, not supported), the desktop text for everything
 * else the flow can answer (an ended attempt, a refused start).
 */
function desktopFailureText(err: unknown): string {
  if (err instanceof PasskeyError && !Object.prototype.hasOwnProperty.call(PASSKEY_ERRORS, err.code)) {
    return t("pcSetup.desktop.failed");
  }
  return passkeyFailureText(err, "pcSetup.desktop.failed", PASSKEY_ERRORS) || t("pcSetup.desktop.failed");
}

/** Whether a failure has its own passkey text, which stays true after the code. */
function hasPasskeyText(err: unknown): boolean {
  return (
    err instanceof PasskeyError &&
    err.code !== "PASSKEY_FAILED" &&
    Object.prototype.hasOwnProperty.call(PASSKEY_ERRORS, err.code)
  );
}

function startFlow(attemptId: string, onState: PcSetupStepProps["onState"]) {
  const old = flows.get(attemptId);
  if (old && !old.done) old.ctl.abort();
  const ctl = new AbortController();
  const [error, setError] = createSignal<string | null>(null);
  const [passkeyText, setPasskeyText] = createSignal(false);
  const flow: BrowserFlow = { ctl, error, passkeyText, done: false };
  flows.set(attemptId, flow);
  setVersion((v) => v + 1);
  setUpThisPcInBrowser(attemptId, ctl.signal).then(
    () => {
      flow.done = true;
      setVersion((v) => v + 1);
      // Redeemed: the passkey exists, also when the step was left meanwhile.
      onState(attemptId, { state: "completed", reason: null });
    },
    (err: unknown) => {
      flow.done = true;
      setVersion((v) => v + 1);
      if (ctl.signal.aborted) return;
      setPasskeyText(hasPasskeyText(err));
      setError(desktopFailureText(err));
    },
  );
}

/** Keep the attempt's flow while a step shows it; end it when none does. */
function attach(attemptId: string) {
  refs.set(attemptId, (refs.get(attemptId) ?? 0) + 1);
  onCleanup(() => {
    const left = (refs.get(attemptId) ?? 1) - 1;
    refs.set(attemptId, left);
    // The next step (code → enrolled) mounts in the same tick: wait for it.
    queueMicrotask(() => {
      if ((refs.get(attemptId) ?? 0) > 0) return;
      refs.delete(attemptId);
      const f = flows.get(attemptId);
      flows.delete(attemptId);
      if (f && !f.done) f.ctl.abort();
    });
  });
}

/** The flow of `attemptId`, alive and not failed. */
const running = (attemptId: string): boolean => {
  version();
  const f = flows.get(attemptId);
  return !!f && !f.done && !f.ctl.signal.aborted;
};

const failure = (attemptId: string): string | null => {
  version();
  return flows.get(attemptId)?.error() ?? null;
};

/** The failure text when it is one of the passkey texts, else null. */
const passkeyFailure = (attemptId: string): string | null => {
  version();
  const f = flows.get(attemptId);
  return f?.passkeyText() ? f.error() : null;
};

/** State `code`: open the browser, where the code is typed. */
export const DesktopCodeStep: Component<PcSetupStepProps> = (props) => {
  // A step belongs to one attempt (the dialog remounts it for another).
  // eslint-disable-next-line solid/reactivity
  const { attemptId, onState } = props;
  attach(attemptId);
  const open = () => startFlow(attemptId, onState);
  return (
    <div class="space-y-3">
      <p class="font-semibold">{t("pcSetup.desktop.title")}</p>
      <p>{t("pcSetup.desktop.intro")}</p>
      <p class="text-sm text-text-primary">{t("pcSetup.codeOwnPhoneOnly")}</p>
      <Show
        when={running(attemptId)}
        fallback={
          <>
            <Show when={failure(attemptId)}>
              <p role="alert" class="text-sm text-text-primary">{failure(attemptId)}</p>
            </Show>
            <button type="button" class="btn-primary min-h-11 text-base" onClick={open}>
              {t("pcSetup.desktop.open")}
            </button>
          </>
        }
      >
        <p role="status" class="text-sm text-text-secondary">{t("pcSetup.desktop.waiting")}</p>
      </Show>
      <LinuxPasskeyHint class="text-sm text-text-secondary" />
    </div>
  );
};

/** State `enrolled`: the passkey is created in the browser the flow opened. */
export const DesktopPasskeyStep: Component<PcSetupStepProps> = (props) => {
  // A step belongs to one attempt (the dialog remounts it for another).
  // eslint-disable-next-line solid/reactivity
  const { attemptId } = props;
  attach(attemptId);
  return (
    <div class="space-y-3">
      <Show
        when={running(attemptId)}
        fallback={
          <>
            {/* After the code one line: the passkey text (limit, already added, …), else Start again. */}
            <p role="alert" class="text-sm text-text-primary">
              {passkeyFailure(attemptId) ?? t("pcSetup.desktop.interrupted")}
            </p>
          </>
        }
      >
        <p>{t("pcSetup.registerIntro")}</p>
        <p role="status" class="text-sm text-text-secondary">{t("pcSetup.desktop.waiting")}</p>
      </Show>
      <LinuxPasskeyHint class="text-sm text-text-secondary" />
    </div>
  );
};

export const DESKTOP_PLATFORM: PcSetupPlatform = { CodeStep: DesktopCodeStep, PasskeyStep: DesktopPasskeyStep };
