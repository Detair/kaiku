/**
 * The OIDC factor of an account request (C27): browser popup with
 * `oidc-request-callback {oidc_code}`, or the desktop's Rust flow. Resolves
 * the one-time code (60 s) for the start body. `signal` aborts either: the
 * popup closes, the desktop flow is cancelled in Rust.
 */

import { requestOidcAuthorize } from "@/lib/tauri/accountRequests";
import { isTauri } from "@/lib/tauri/common";
import { openBlankSsoPopup } from "@/lib/oidcPopup";

/**
 * Only an `https:` provider page (or `http:` on a loopback host) goes into
 * the same-origin popup; anything else (`javascript:`, `data:`) is refused
 * (Argon review, part 5 #5).
 */
export function safeProviderUrl(raw: string, base: string): string | null {
  let url: URL;
  try {
    url = new URL(raw, base);
  } catch {
    return null;
  }
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  const ok = url.protocol === "https:" || (url.protocol === "http:" && loopback);
  return ok && !url.username && !url.password ? url.href : null;
}

export async function requestOidcCode(serverUrl: string, providerId: string, signal?: AbortSignal): Promise<string> {
  const popup = isTauri() ? null : openBlankSsoPopup("oidc-request");
  if (!isTauri() && !popup) throw new Error("POPUP_BLOCKED");
  try {
    const r = await requestOidcAuthorize(serverUrl, providerId, signal);
    if (r.mode === "tauri") return r.oidcCode;
    const expectedOrigin = new URL(serverUrl).origin;
    const target = safeProviderUrl(r.url, expectedOrigin);
    if (!target) throw new Error("OIDC_FAILED");
    if (signal?.aborted) throw new Error("ABORTED");
    return await new Promise<string>((resolve, reject) => {
      const timer = setInterval(() => {
        if (popup!.closed) {
          cleanup();
          reject(new Error("POPUP_CLOSED"));
        }
      }, 500);
      const onAbort = () => {
        cleanup();
        popup?.close();
        reject(new Error("ABORTED"));
      };
      const onMessage = (event: MessageEvent) => {
        if (event.origin !== expectedOrigin || event.source !== popup) return;
        const data = event.data as { type?: unknown; oidc_code?: unknown } | null;
        if (data?.type === "oidc-request-callback" && typeof data.oidc_code === "string") {
          cleanup();
          popup?.close();
          resolve(data.oidc_code);
        }
      };
      const cleanup = () => {
        clearInterval(timer);
        window.removeEventListener("message", onMessage);
        signal?.removeEventListener("abort", onAbort);
      };
      window.addEventListener("message", onMessage);
      signal?.addEventListener("abort", onAbort, { once: true });
      popup!.location.href = target;
    });
  } catch (err) {
    popup?.close();
    throw err;
  }
}
