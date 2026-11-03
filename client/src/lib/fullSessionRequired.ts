/**
 * Open state of the full-session dialog (C27: a request refused with 403
 * `FULL_SESSION_REQUIRED`). It lives outside the dialog, which is a lazy
 * chunk (bundle budget), so a refusal that arrives before that chunk has
 * loaded still opens it.
 */

import { createSignal } from "solid-js";
import { FULL_SESSION_REQUIRED_EVENT } from "@/lib/tauri/common";

const [open, setOpen] = createSignal(false);

/** Whether the full-session dialog is open (reactive). */
export const fullSessionRequiredOpen = open;

/** Open the dialog (used by `withStepUp` for a chat-only session). */
export function showFullSessionRequired(): void {
  setOpen(true);
}

export function closeFullSessionRequired(): void {
  setOpen(false);
}

// `nativeSessionRequest` / `httpRequest` dispatch this (at most once per 2 s).
if (typeof window !== "undefined") {
  window.addEventListener(FULL_SESSION_REQUIRED_EVENT, showFullSessionRequired);
}
