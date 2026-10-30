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

/**
 * Wait for the callback page's postMessage. `onResult` fires for a valid
 * message from `expectedOrigin`; `onClosedWithoutResult` fires when the popup
 * closes first (for example when an IdP severed `window.opener`). Returns a
 * disposer that stops all listening.
 */
export function watchSsoPopup(opts: {
  popup: Window;
  expectedOrigin: string;
  onResult: (data: OidcCallbackData) => void;
  onClosedWithoutResult: () => void;
}): () => void {
  const stop = () => {
    clearInterval(timer);
    window.removeEventListener("message", handler);
  };
  const handler = (event: MessageEvent) => {
    // Validate origin to prevent cross-origin token theft
    if (event.origin !== opts.expectedOrigin) return;
    if (event.data?.type === "oidc-callback" && event.data.access_token) {
      stop();
      opts.onResult(event.data as OidcCallbackData);
    }
  };
  const timer = setInterval(() => {
    if (opts.popup.closed) {
      stop();
      opts.onClosedWithoutResult();
    }
  }, 500);
  window.addEventListener("message", handler);
  return stop;
}
