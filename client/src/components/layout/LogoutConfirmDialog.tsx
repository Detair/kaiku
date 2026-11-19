/**
 * LogoutConfirmDialog - asks before signing out (NAV-9, KONTO-7).
 *
 * Signing out drops unsent drafts and, on a PC, often needs the phone to get
 * back in, so one stray click next to the gear must not do it. "Cancel" has
 * the focus, so Enter keeps the user signed in.
 */

import { Component } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "@/i18n";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

interface LogoutConfirmDialogProps {
  onConfirm: () => void;
  onCancel: () => void;
}

const LogoutConfirmDialog: Component<LogoutConfirmDialogProps> = (props) => {
  let cancelButton: HTMLButtonElement | undefined;
  const dialog = createDialog({
    onClose: () => props.onCancel(),
    initialFocus: () => cancelButton,
  });
  // Above every window already open: this one takes the keyboard.
  const layer = createModalLayer(MODAL_LAYER.prompt);

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center"
        style={{ "z-index": layer }}
        onClick={() => props.onCancel()}
      >
        <div
          ref={dialog.ref}
          {...dialog.dialogProps}
          data-testid="logout-dialog"
          class="border border-white/10 rounded-2xl w-full max-w-sm mx-4 p-6 outline-none"
          style={{ "background-color": "var(--color-surface-base)" }}
          onClick={(e) => e.stopPropagation()}
        >
          <h2 id={dialog.titleId} class="text-lg font-bold text-text-primary mb-2">
            {t("home.logout.title")}
          </h2>
          <p class="text-sm text-text-secondary mb-6">{t("home.logout.body")}</p>
          <div class="flex gap-3 justify-end">
            <button
              ref={cancelButton}
              type="button"
              class="px-4 py-2 rounded-lg text-text-primary bg-white/10 hover:bg-white/20 transition-colors"
              onClick={() => props.onCancel()}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              data-testid="logout-confirm"
              class="btn-danger"
              onClick={() => props.onConfirm()}
            >
              {t("home.logout.confirm")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default LogoutConfirmDialog;
