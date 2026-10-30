/**
 * Browser SSO popup helpers shared by the sign-in and register views.
 *
 * The popup must be opened synchronously inside the click handler (before any
 * `await`), otherwise popup blockers reject it. It starts blank and is pointed
 * at the authorize URL once the server has answered.
 */

let popupAttempt = 0;

/** Open the blank SSO window. Returns null when the browser blocked it. */
export function openBlankSsoPopup(name: string): Window | null {
  return window.open("about:blank", `${name}-${Date.now()}-${++popupAttempt}`, "width=600,height=700");
}

export interface OidcCallbackData {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  setup_required?: boolean;
}

/** How long to keep listening after the popup closes (message vs close race). */
export const CLOSE_GRACE_MS = 1000;

/**
 * Wait for the callback page's postMessage. `onResult` fires for a valid
 * message from `expectedOrigin` sent by `popup` itself (other same-origin
 * windows must not be able to inject a login). `onClosedWithoutResult` fires
 * when the popup closed and no message arrived within a short grace period
 * (the callback posts its result and then closes, so the close can be seen
 * first), for example when an IdP severed `window.opener`. At most one of the
 * two callbacks runs. Returns a disposer that stops all listening.
 */
export function watchSsoPopup(opts: {
  popup: Window;
  expectedOrigin: string;
  onResult: (data: OidcCallbackData) => void;
  onClosedWithoutResult: () => void;
}): () => void {
  let settled = false;
  let grace: ReturnType<typeof setTimeout> | undefined;
  const stop = () => {
    settled = true;
    clearInterval(timer);
    clearTimeout(grace);
    window.removeEventListener("message", handler);
  };
  const handler = (event: MessageEvent) => {
    if (settled) return;
    // Validate origin and sender to prevent token injection
    if (event.origin !== opts.expectedOrigin || event.source !== opts.popup) return;
    if (event.data?.type === "oidc-callback" && event.data.access_token) {
      stop();
      opts.onResult(event.data as OidcCallbackData);
    }
  };
  const timer = setInterval(() => {
    if (!opts.popup.closed) return;
    clearInterval(timer);
    grace = setTimeout(() => {
      if (settled) return;
      stop();
      opts.onClosedWithoutResult();
    }, CLOSE_GRACE_MS);
  }, 500);
  window.addEventListener("message", handler);
  return stop;
}

/** Retry interval and overall cap while waiting for a sign-in finished elsewhere. */
export const RESTORE_POLL_MS = 2500;
export const RESTORE_WAIT_CAP_MS = 5 * 60 * 1000;

/**
 * After the popup was reported closed without a result (an IdP with COOP can
 * make it look closed while the user is still signing in), keep trying to
 * resume the session: now, on window focus / tab visible, and on an interval,
 * until it works, the cap passes, or the returned disposer runs.
 * `onDone(true)` = session restored; `onDone(false)` = gave up at the cap.
 * Disposing never calls `onDone`. Attempts never overlap; each gets a signal
 * that is aborted when the wait ends (dispose, cap, success), so a late
 * response is not applied.
 */
export function waitForSessionRestore(opts: {
  restore: (signal: AbortSignal) => Promise<boolean>;
  onDone: (restored: boolean) => void;
}): () => void {
  let settled = false;
  let inFlight = false;
  const abort = new AbortController();
  const stop = () => {
    settled = true;
    abort.abort();
    clearInterval(poll);
    clearTimeout(cap);
    window.removeEventListener("focus", attempt);
    document.removeEventListener("visibilitychange", onVisible);
  };
  const attempt = () => {
    if (settled || inFlight) return;
    inFlight = true;
    void opts
      .restore(abort.signal)
      .catch(() => false)
      .then((ok) => {
        inFlight = false;
        if (settled || !ok) return;
        stop();
        opts.onDone(true);
      });
  };
  const onVisible = () => {
    if (document.visibilityState === "visible") attempt();
  };
  const poll = setInterval(attempt, RESTORE_POLL_MS);
  const cap = setTimeout(() => {
    if (settled) return;
    stop();
    opts.onDone(false);
  }, RESTORE_WAIT_CAP_MS);
  window.addEventListener("focus", attempt);
  document.addEventListener("visibilitychange", onVisible);
  attempt();
  return stop;
}
