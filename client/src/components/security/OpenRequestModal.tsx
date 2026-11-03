/**
 * OpenRequestModal — a request to change how this account signs in is
 * waiting (SM3): shown full-screen at app start (and after a reconnect) with
 * a large "Cancel this request"; "That's right" closes it until the next app
 * start. Never snoozed.
 */

import { Component, For, Show, createSignal } from "solid-js";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { Portal } from "solid-js/web";
import { ShieldAlert } from "lucide-solid";
import { t } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";
import { cancelAccountRequest, type Part2Factor } from "@/lib/tauri/security";
import { withStepUp, StepUpCancelled } from "@/lib/stepUp";
import {
  dismissOpenRequest,
  isOpenRequestDismissed,
  meSecurity,
  refreshMeSecurity,
  clearOpenRequestDismissals,
} from "@/stores/security";
import Part2FactorDialog from "./Part2FactorDialog";
import { noticeActionError } from "./runNoticeAction";

/** Tests: a new app start (a new session clears them too, `resetMeSecurity`). */
export function resetOpenRequestDismissals(): void {
  clearOpenRequestDismissals();
}

const OpenRequestDialog: Component<{ close: () => void }> = (props) => {
  // Escape does not answer "That's right": an account-change warning needs
  // an explicit click (Argon review 2 #3).
  const dialog = createDialog({ onClose: () => undefined });
  // Its part-2 and step-up confirmations open above it (`DialogShell`).
  const layer = createModalLayer(MODAL_LAYER.prompt);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [part2, setPart2] = createSignal(false);
  const [tooLate, setTooLate] = createSignal(false);
  const req = () => meSecurity()!.open_request!;

  const cancel = async (factor: Part2Factor | null) => {
    setBusy(true);
    setError(null);
    try {
      const id = req().id;
      if (req().cancel?.via === "part2") await cancelAccountRequest(id, null, factor);
      else
        await withStepUp({ action: "request.cancel", target: id, targetLabel: req().subject.display_name }, () =>
          cancelAccountRequest(id, null, null),
        );
      await refreshMeSecurity();
    } catch (err) {
      if (err instanceof StepUpCancelled) return;
      if (err instanceof HttpError && codeIs(err.code, "ACCOUNT_REQUEST_COMPLETED")) {
        // Lost against completion (Argon review 2 #2): say so and show the
        // account as it is now; the new device is in Security settings.
        setTooLate(true);
        await refreshMeSecurity();
        return;
      }
      setError(noticeActionError(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      class="fixed inset-0 flex items-center justify-center p-4"
      style={{ "background-color": "var(--color-surface-base)", "z-index": layer }}
    >
      <div class="w-full max-w-lg space-y-5" {...dialog.dialogProps} ref={dialog.ref}>
        <div class="flex items-start gap-3">
          <ShieldAlert class="w-8 h-8 shrink-0 text-status-warning" aria-hidden="true" />
          <h2 id={dialog.titleId} class="text-2xl font-bold text-text-primary">
            {t("security.request.prompt")}
          </h2>
        </div>
        <Show when={req().kind === "admin" && req().started_by}>
          {(admin) => <p class="text-lg text-text-primary">{t("security.request.adminSetup", { admin: admin().username })}</p>}
        </Show>
        <dl class="space-y-1 text-base text-text-primary">
          <div>
            <dt class="inline text-text-secondary">{t("security.request.started")} </dt>
            <dd class="inline">{formatTimestamp(req().created_at)}</dd>
          </div>
          <div>
            <dt class="inline text-text-secondary">{t("security.request.readyAt")} </dt>
            <dd class="inline">{formatTimestamp(req().ready_at)}</dd>
          </div>
        </dl>
        <Show when={req().lost_devices.length > 0}>
          <div>
            <p class="text-base text-text-secondary">{t("security.request.lostDevices")}</p>
            <ul class="list-disc list-inside text-base text-text-primary">
              <For each={req().lost_devices}>{(d) => <li>{d.label}</li>}</For>
            </ul>
          </div>
        </Show>
        <Show when={error()}>
          <p role="alert" class="text-base text-text-primary">{error()}</p>
        </Show>
        <Show when={tooLate()}>
          <p role="status" class="text-base text-text-primary">{t("security.action.tooLate")}</p>
        </Show>
        <Show when={req().cancel}>
          <button
            type="button"
            class="btn-danger w-full min-h-14 text-lg"
            disabled={busy()}
            onClick={() => (req().cancel?.via === "part2" ? setPart2(true) : void cancel(null))}
          >
            {t("security.request.cancel")}
          </button>
        </Show>
        <button
          type="button"
          class="w-full min-h-12 rounded-lg border border-white/15 text-base text-text-primary hover:bg-white/5"
          onClick={() => props.close()}
        >
          {t("security.request.thatsRight")}
        </button>
        <Show when={part2()}>
          <Part2FactorDialog
            onClose={() => setPart2(false)}
            onSubmit={(factor) => {
              setPart2(false);
              void cancel(factor);
            }}
          />
        </Show>
      </div>
    </div>
  );
};

const OpenRequestModal: Component = () => {
  const open = () => {
    const r = meSecurity()?.open_request;
    return !!r && (r.state === "waiting" || r.state === "ready") && !isOpenRequestDismissed(r.id, r.state);
  };
  const close = () => {
    const r = meSecurity()?.open_request;
    if (r) dismissOpenRequest(r.id, r.state);
  };
  return (
    <Show when={open()}>
      <Portal>
        <OpenRequestDialog close={close} />
      </Portal>
    </Show>
  );
};

export default OpenRequestModal;
