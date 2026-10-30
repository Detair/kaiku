/**
 * Feature detection for browser-mode voice features.
 *
 * In the Tauri shell these are handled natively, so every check reports
 * `true` there; they only gate the UI in the browser.
 */

import { isTauri } from "@/lib/tauri/common";

/** Screen capture needs `getDisplayMedia` (missing on most mobile browsers). */
export function canShareScreen(): boolean {
  if (isTauri()) return true;
  return typeof navigator !== "undefined" && !!navigator.mediaDevices?.getDisplayMedia;
}

/** Choosing a speaker needs `HTMLMediaElement.setSinkId` (not in Safari/Firefox). */
export function canSelectOutputDevice(): boolean {
  if (isTauri()) return true;
  return (
    typeof HTMLMediaElement !== "undefined" &&
    typeof HTMLMediaElement.prototype.setSinkId === "function"
  );
}
