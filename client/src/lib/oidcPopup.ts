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
