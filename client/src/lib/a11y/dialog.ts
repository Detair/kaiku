/**
 * Dialog keyboard and focus basics.
 *
 * `createDialog` gives a modal the behaviour screen-reader and keyboard users
 * expect: Escape closes it (and goes no further), Tab and Shift+Tab stay
 * inside it, focus moves in on open and returns to the opener on close.
 *
 * Spread `dialogProps` on the dialog element, pass `ref` as its `ref`, and put
 * `id={titleId}` on its visible heading.
 */

import { createUniqueId, onCleanup, onMount } from "solid-js";

export interface DialogOptions {
  onClose: () => void;
  /** Element to focus on open. Defaults to the first focusable, else the dialog. */
  initialFocus?: () => HTMLElement | undefined;
}

export interface DialogHandle {
  ref: (el: HTMLElement) => void;
  titleId: string;
  dialogProps: {
    role: "dialog";
    "aria-modal": "true";
    "aria-labelledby": string;
    tabIndex: -1;
  };
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableIn(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute("hidden") && el.getAttribute("aria-hidden") !== "true",
  );
}

export function createDialog(opts: DialogOptions): DialogHandle {
  const titleId = `dialog-title-${createUniqueId()}`;
  // Component setup runs while the opener (button, menu item) still has focus.
  const opener =
    typeof document !== "undefined" && document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  let el: HTMLElement | undefined;

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      opts.onClose();
      return;
    }
    if (e.key !== "Tab" || !el) return;
    const items = focusableIn(el);
    if (items.length === 0) {
      e.preventDefault();
      el.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (!active || !el.contains(active) || active === el) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
    } else if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  };

  const focusIn = () => {
    if (!el) return;
    const target = opts.initialFocus?.() ?? focusableIn(el)[0] ?? el;
    target.focus();
  };

  const ref = (node: HTMLElement) => {
    el = node;
    node.addEventListener("keydown", onKeyDown);
  };

  onMount(() => {
    // Inside a Portal the node may not be attached yet when onMount fires.
    if (el?.isConnected) focusIn();
    else queueMicrotask(focusIn);
  });

  onCleanup(() => {
    el?.removeEventListener("keydown", onKeyDown);
    if (opener?.isConnected) opener.focus();
  });

  return {
    ref,
    titleId,
    dialogProps: {
      role: "dialog",
      "aria-modal": "true",
      "aria-labelledby": titleId,
      tabIndex: -1,
    },
  };
}

/**
 * True when an Escape keydown belongs to an open dialog, context menu or
 * search overlay, so page-level Escape shortcuts must leave it alone.
 */
export function isEscapeForOverlay(e: KeyboardEvent): boolean {
  if (e.defaultPrevented) return true;
  const target = e.target instanceof Element ? e.target : null;
  if (target?.closest("[role='dialog'], [data-context-menu], [data-search-overlay]")) {
    return true;
  }
  return document.querySelector("[role='dialog'], [data-context-menu], [data-search-overlay]") !== null;
}
