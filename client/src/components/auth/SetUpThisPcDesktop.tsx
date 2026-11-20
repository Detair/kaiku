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
import { setUpThisPcInBrowser } from "@/lib/tauri/passkeys";
import type { PcSetupPlatform, PcSetupStepProps } from "./SetUpThisPc";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

interface BrowserFlow {
  ctl: AbortController;
  error: () => string | null;
  /** The command has returned (nothing left to cancel). */
  done: boolean;
}

const flows = new Map<string, BrowserFlow>();
/** Steps showing each attempt's flow. */
const refs = new Map<string, number>();
const [version, setVersion] = createSignal(0);

function startFlow(attemptId: string, onState: PcSetupStepProps["onState"]) {
  const old = flows.get(attemptId);
  if (old && !old.done) old.ctl.abort();
  const ctl = new AbortController();
  const [error, setError] = createSignal<string | null>(null);
  const flow: BrowserFlow = { ctl, error, done: false };
  flows.set(attemptId, flow);
  setVersion((v) => v + 1);
  setUpThisPcInBrowser(attemptId, ctl.signal).then(
    () => {
      flow.done = true;
      setVersion((v) => v + 1);
      if (!ctl.signal.aborted) onState(attemptId, { state: "completed", reason: null });
    },
    (err: unknown) => {
      flow.done = true;
      setVersion((v) => v + 1);
      if (ctl.signal.aborted) return;
      setError(passkeyFailureText(err, "auth.pcSetup.desktop.failed", PASSKEY_ERRORS) || t("auth.pcSetup.desktop.failed"));
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

/** State `code`: open the browser, where the code is typed. */
export const DesktopCodeStep: Component<PcSetupStepProps> = (props) => {
  // A step belongs to one attempt (the dialog remounts it for another).
  // eslint-disable-next-line solid/reactivity
  const { attemptId, onState } = props;
  attach(attemptId);
  const open = () => startFlow(attemptId, onState);
  return (
    <div class="space-y-3">
      <p class="font-semibold">{t("auth.pcSetup.desktop.title")}</p>
      <p>{t("auth.pcSetup.desktop.intro")}</p>
      <p class="text-sm text-text-primary">{t("auth.pcSetup.codeWarning")}</p>
      <Show
        when={running(attemptId)}
        fallback={
          <>
            <Show when={failure(attemptId)}>
              <p role="alert" class="text-sm text-text-primary">{failure(attemptId)}</p>
            </Show>
            <button type="button" class="btn-primary min-h-11 text-base" onClick={open}>
              {t("auth.pcSetup.desktop.open")}
            </button>
          </>
        }
      >
        <p role="status" class="text-sm text-text-secondary">{t("auth.pcSetup.desktop.waiting")}</p>
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
            <p role="alert" class="text-sm text-text-primary">
              {failure(attemptId) ?? t("auth.pcSetup.desktop.interrupted")}
            </p>
            <Show when={failure(attemptId)}>
              <p class="text-sm text-text-secondary">{t("auth.pcSetup.desktop.interrupted")}</p>
            </Show>
          </>
        }
      >
        <p>{t("auth.pcSetup.registerIntro")}</p>
        <p role="status" class="text-sm text-text-secondary">{t("auth.pcSetup.desktop.waiting")}</p>
      </Show>
      <LinuxPasskeyHint class="text-sm text-text-secondary" />
    </div>
  );
};

export const DESKTOP_PLATFORM: PcSetupPlatform = { CodeStep: DesktopCodeStep, PasskeyStep: DesktopPasskeyStep };
