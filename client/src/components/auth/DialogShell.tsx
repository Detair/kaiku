/**
 * DialogShell — the modal frame of the v1.0 sign-in dialogs: overlay, title,
 * close button, keyboard and focus handling (`createDialog`), and a layer
 * above every modal already open (`createModalLayer`). Readable text
 * uses `text-text-primary` / `text-text-secondary` (CLAUDE.md contrast rules).
 */

import { Component, JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { X } from "lucide-solid";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";
import { t } from "@/i18n";

export interface DialogShellProps {
  title: string;
  onClose: () => void;
  children: JSX.Element;
  /** Wider dialogs (setup codes, notices). */
  wide?: boolean;
  testId?: string;
}

const DialogShell: Component<DialogShellProps> = (props) => {
  const dialog = createDialog({ onClose: () => props.onClose() });
  // Above any modal already open (a confirmation from a full-screen prompt).
  const layer = createModalLayer(MODAL_LAYER.dialog);
  return (
    <Portal>
      <div
        class="fixed inset-0 flex items-center justify-center bg-black/60 p-4"
        style={{ "z-index": layer }}
        onClick={() => props.onClose()}
      >
        <div
          class={`w-full ${props.wide ? "max-w-lg" : "max-w-md"} max-h-[90dvh] overflow-y-auto rounded-xl shadow-2xl border border-white/10`}
          style={{ "background-color": "var(--color-surface-layer1)" }}
          data-testid={props.testId}
          {...dialog.dialogProps}
          ref={dialog.ref}
          onClick={(e) => e.stopPropagation()}
        >
          <div class="flex items-center justify-between gap-3 p-4 border-b border-white/10">
            <h2 id={dialog.titleId} class="text-lg font-semibold text-text-primary">
              {props.title}
            </h2>
            <button
              type="button"
              onClick={() => props.onClose()}
              aria-label={t("common.close")}
              class="p-2 min-w-11 min-h-11 flex items-center justify-center hover:bg-white/10 rounded-lg transition-colors text-text-secondary"
            >
              <X class="w-5 h-5" />
            </button>
          </div>
          <div class="p-4 space-y-4 text-base text-text-primary">{props.children}</div>
        </div>
      </div>
    </Portal>
  );
};

export default DialogShell;
