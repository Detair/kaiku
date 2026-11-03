/**
 * The OIDC factor of an account request (C27): browser popup with
 * `oidc-request-callback {oidc_code}`, or the desktop's Rust flow. Resolves
 * the one-time code (60 s) for the start body.
 */

import { requestOidcAuthorize } from "@/lib/tauri/accountRequests";
import { isTauri } from "@/lib/tauri/common";
import { openBlankSsoPopup } from "@/lib/oidcPopup";

export async function requestOidcCode(serverUrl: string, providerId: string): Promise<string> {
  const popup = isTauri() ? null : openBlankSsoPopup("oidc-request");
  if (!isTauri() && !popup) throw new Error("POPUP_BLOCKED");
  try {
    const r = await requestOidcAuthorize(serverUrl, providerId);
    if (r.mode === "tauri") return r.oidcCode;
    const expectedOrigin = new URL(serverUrl).origin;
    return await new Promise<string>((resolve, reject) => {
      const timer = setInterval(() => {
        if (popup!.closed) {
          cleanup();
          reject(new Error("POPUP_CLOSED"));
        }
      }, 500);
      const onMessage = (event: MessageEvent) => {
        if (event.origin !== expectedOrigin || event.source !== popup) return;
        const data = event.data as { type?: unknown; oidc_code?: unknown } | null;
        if (data?.type === "oidc-request-callback" && typeof data.oidc_code === "string") {
          cleanup();
          resolve(data.oidc_code);
        }
      };
      const cleanup = () => {
        clearInterval(timer);
        window.removeEventListener("message", onMessage);
      };
      window.addEventListener("message", onMessage);
      popup!.location.href = r.url;
    });
  } catch (err) {
    popup?.close();
    throw err;
  }
}
