/**
 * Eager state of the session dialogs, which are a lazy chunk (bundle
 * budget; `App.tsx` imports this module at startup):
 * - the open state of the full-session dialog (C27: a request refused with
 *   403 `FULL_SESSION_REQUIRED`), so a refusal that arrives before the chunk
 *   has loaded still opens it;
 * - whether that chunk failed to load, so a step-up fails instead of waiting
 *   for a dialog that never comes (Fable r2 R1).
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

let dialogsFailed = false;
const failedListeners = new Set<() => void>();

/** The session dialogs' chunk could not load (their error boundary says so). */
export function markSessionDialogsFailed(): void {
  if (dialogsFailed) return;
  dialogsFailed = true;
  for (const listener of failedListeners) listener();
}

export function sessionDialogsFailed(): boolean {
  return dialogsFailed;
}

/** Run `listener` once the session dialogs' chunk is known to have failed. */
export function onSessionDialogsFailed(listener: () => void): void {
  failedListeners.add(listener);
}

// `nativeSessionRequest` / `httpRequest` dispatch this (at most once per 2 s).
if (typeof window !== "undefined") {
  window.addEventListener(FULL_SESSION_REQUIRED_EVENT, showFullSessionRequired);
}
