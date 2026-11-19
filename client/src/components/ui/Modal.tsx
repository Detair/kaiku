/**
 * Modal — backdrop and panel for any modal dialog that brings its own layout.
 *
 * Wraps `createDialog` and `createModalLayer` (A11Y-1, NAV-11): the panel is a
 * `role="dialog"` with a name, focus moves in on open, Tab stays inside,
 * Escape closes from wherever focus was (a document listener, not one on the
 * panel), focus returns to the opener, and a dialog opened from another one
 * sits above it and alone reacts to the keyboard.
 *
 * Name the dialog with `labelledBy` (id of its visible heading, from
 * `createUniqueId()`) or with `label` when it has no heading.
 * `DialogShell` (components/auth) is the ready-made frame with title and close
 * button; use this one when the dialog draws its own header.
 */

import { Component, JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

export interface ModalProps {
  /** Escape, backdrop click. Put guards (unsaved changes) in here. */
  onClose: () => void;
  /** Id of the visible heading. */
  labelledBy?: string;
  /** Accessible name when there is no visible heading. */
  label?: string;
  /** Id of a short description (warning text, question). */
  describedBy?: string;
  /** Classes of the panel. */
  class?: string;
  style?: JSX.CSSProperties;
  /** Classes of the backdrop (layout and colour). */
  backdropClass?: string;
  /** Backdrop click closes (default true). */
  closeOnBackdrop?: boolean;
  /** Element to focus on open; default the first focusable element. */
  initialFocus?: () => HTMLElement | undefined;
  /** z-index base; default the dialog layer. */
  layer?: number;
  testId?: string;
  children: JSX.Element;
}

const Modal: Component<ModalProps> = (props) => {
  const dialog = createDialog({
    onClose: () => props.onClose(),
    initialFocus: () => props.initialFocus?.(),
  });
  // Fixed for the dialog's lifetime: the layer is taken once on open.
  // eslint-disable-next-line solid/reactivity
  const z = createModalLayer(props.layer ?? MODAL_LAYER.dialog);

  return (
    <Portal>
      <div
        class={
          props.backdropClass ??
          "fixed inset-0 flex items-center justify-center bg-black/60 p-4"
        }
        style={{ "z-index": z }}
        data-modal-backdrop
        onClick={(e) => {
          if (e.target === e.currentTarget && props.closeOnBackdrop !== false) {
            props.onClose();
          }
        }}
      >
        <div
          {...dialog.dialogProps}
          aria-labelledby={props.label ? undefined : (props.labelledBy ?? dialog.titleId)}
          aria-label={props.label}
          aria-describedby={props.describedBy}
          ref={dialog.ref}
          class={props.class}
          style={props.style}
          data-testid={props.testId}
        >
          {props.children}
        </div>
      </div>
    </Portal>
  );
};

export default Modal;
