/**
 * ConfirmActionDialog — one confirmation for admin and server-settings
 * actions (ADMIN-13): names the target and the consequence, runs the action,
 * and stays open with the reason when it fails (ADMIN-1). The caller closes
 * it from `onDone` after a success.
 */
import { Component, JSX, Show, createSignal } from "solid-js";
import DialogShell from "@/components/auth/DialogShell";
import { actionErrorText } from "./actionErrors";
import { t, type MessageKey } from "@/i18n";

export interface ConfirmActionDialogProps {
  title: string;
  /** What happens, naming the target. */
  message: JSX.Element;
  confirmLabel: string;
  /** Red button for actions that remove or block something. */
  danger?: boolean;
  /** Extra fields (a reason, a typed name). */
  children?: JSX.Element;
  confirmDisabled?: boolean;
  /** Throws on failure; the dialog then shows why. */
  onConfirm: () => Promise<void>;
  onDone: () => void;
  onCancel: () => void;
  failureFallback?: MessageKey;
  testId?: string;
}

const ConfirmActionDialog: Component<ConfirmActionDialogProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const confirm = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await props.onConfirm();
      props.onDone();
    } catch (err) {
      setError(actionErrorText(err, props.failureFallback));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell title={props.title} onClose={() => !busy() && props.onCancel()} testId={props.testId}>
      <div class="text-sm text-text-primary">{props.message}</div>
      {props.children}
      <Show when={error()}>
        <p
          role="alert"
          class="p-3 rounded-lg bg-status-error/15 border border-status-error/50 text-sm text-text-primary"
        >
          {error()}
        </p>
      </Show>
      <div class="flex gap-3 pt-2">
        <button
          type="button"
          onClick={() => props.onCancel()}
          disabled={busy()}
          class="flex-1 px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20 state-disabled"
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          onClick={confirm}
          disabled={busy() || props.confirmDisabled}
          class="flex-1 px-4 py-2 rounded-lg font-medium transition-colors state-disabled"
          classList={{
            "bg-accent-danger-fill text-on-danger hover:bg-accent-danger-fill/90": !!props.danger,
            "bg-accent-primary text-on-accent hover:bg-accent-primary/90": !props.danger,
          }}
        >
          {busy() ? t("guildSettings.working") : props.confirmLabel}
        </button>
      </div>
    </DialogShell>
  );
};

export default ConfirmActionDialog;
