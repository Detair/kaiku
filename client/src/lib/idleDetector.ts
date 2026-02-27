/**
 * Idle Detection Module
 *
 * Tracks user activity (mouse, keyboard, scroll, touch) and triggers
 * idle status after configurable timeout.
 */

type IdleCallback = (isIdle: boolean) => void;

let idleTimeout: number | null = null;
let isCurrentlyIdle = false;
let callback: IdleCallback | null = null;
let timeoutMs = 5 * 60 * 1000; // 5 minutes default

function resetIdleTimer(): void {
  if (idleTimeout) clearTimeout(idleTimeout);

  if (isCurrentlyIdle) {
    isCurrentlyIdle = false;
    callback?.(false);
  }

  idleTimeout = window.setTimeout(() => {
    isCurrentlyIdle = true;
    callback?.(true);
  }, timeoutMs);
}

const events = ["mousedown", "mousemove", "keydown", "scroll", "touchstart"];

/**
 * Start idle detection with the given callback.
 * The callback is called with `true` when the user becomes idle,
 * and `false` when the user becomes active again.
 *
 * @param onIdleChange - Callback invoked when idle state changes
 * @param timeoutMinutes - Minutes of inactivity before user is considered idle (default: 5)
 */
export function startIdleDetection(
  onIdleChange: IdleCallback,
  timeoutMinutes = 5,
): void {
  callback = onIdleChange;
  timeoutMs = timeoutMinutes * 60 * 1000;

  events.forEach((event) => {
    document.addEventListener(event, resetIdleTimer, { passive: true });
  });

  resetIdleTimer();
}

/**
 * Stop idle detection and clean up event listeners.
 */
export function stopIdleDetection(): void {
  events.forEach((event) => {
    document.removeEventListener(event, resetIdleTimer);
  });

  if (idleTimeout) {
    clearTimeout(idleTimeout);
    idleTimeout = null;
  }

  callback = null;
  isCurrentlyIdle = false;
}

/**
 * Update the idle timeout duration.
 * If detection is active, the timer will be reset with the new duration.
 *
 * @param minutes - New timeout in minutes
 */
export function setIdleTimeout(minutes: number): void {
  timeoutMs = minutes * 60 * 1000;
  if (callback) {
    resetIdleTimer();
  }
}

/**
 * Check if the user is currently idle.
 */
export function isIdle(): boolean {
  return isCurrentlyIdle;
}
