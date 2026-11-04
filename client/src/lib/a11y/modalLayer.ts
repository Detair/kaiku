/**
 * Stacking order of modal overlays.
 *
 * A modal that opens while others are open goes above all of them, whatever
 * their base layers: a confirmation opened from the full-screen open-request
 * prompt (base 80) must not render behind it with its own base (70), where
 * pointer users could not reach it and keyboard focus would be invisible.
 * Call in the modal's component body; the layer is released on cleanup.
 */

import { onCleanup } from "solid-js";

/** Modal overlay bases (z-index). */
export const MODAL_LAYER = {
  /** Dialogs (`DialogShell`). */
  dialog: 70,
  /** The full-screen open-request prompt. */
  prompt: 80,
} as const;

const open: number[] = [];

/** The z-index for a modal opening now: `base`, or above every open modal. */
export function createModalLayer(base: number): number {
  const z = open.reduce((top, layer) => Math.max(top, layer + 1), base);
  open.push(z);
  onCleanup(() => {
    const i = open.indexOf(z);
    if (i >= 0) open.splice(i, 1);
  });
  return z;
}
