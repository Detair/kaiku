/**
 * The server of the flows that run without a session (Codex review M3):
 * recovery, "Secure my account", the request client after a restart.
 *
 * On the desktop a fresh start has no session, so the transport knows no
 * server; the sign-in page's choice (its build default or the native
 * override `get_default_server_url`) is kept here for this run. A signed-in
 * account always uses its own session's server instead, and nothing here
 * changes where the session's bearer goes.
 */

import { getServerUrl, isTauri } from "@/lib/tauri/common";
import { getDefaultServerUrl } from "@/lib/tauri/settings";

const KEY = "kaiku:sign-in-server";
let chosen: string | null = null;

function baseOf(url: string): string {
  return url.trim().replace(/\/+$/, "");
}

function isHttpUrl(url: string | null | undefined): url is string {
  return !!url && /^https?:\/\//i.test(url);
}

/** The sign-in page names the server it signs in to (also the native override). */
export function setSignInServer(url: string): void {
  if (!isHttpUrl(url)) return;
  chosen = baseOf(url);
  try {
    sessionStorage.setItem(KEY, chosen);
  } catch {
    // Storage blocked: the in-memory choice still holds for this run.
  }
}

function stored(): string | null {
  try {
    const v = sessionStorage.getItem(KEY);
    return isHttpUrl(v) ? v : null;
  } catch {
    return null;
  }
}

/** The build-time default the sign-in page starts with. */
function buildDefault(): string {
  const env = import.meta.env.VITE_SERVER_URL as string | undefined;
  if (isHttpUrl(env)) return baseOf(env);
  return typeof window !== "undefined" ? window.location.origin : "";
}

/**
 * The server for a pre-session flow now: the session's own server when
 * `signedIn`, else the sign-in page's choice, else the transport's or the
 * build default. Never the desktop app's own `tauri://` origin.
 */
export function signInServer(signedIn = false): string {
  const session = getServerUrl();
  if (signedIn && isHttpUrl(session)) return baseOf(session);
  const choice = chosen ?? stored();
  if (choice) return choice;
  if (isHttpUrl(session)) return baseOf(session);
  return buildDefault();
}

/** Like `signInServer`, also asking for the native override when nothing was chosen yet. */
export async function resolveSignInServer(signedIn = false): Promise<string> {
  if (!signedIn && !chosen && !stored() && isTauri()) {
    try {
      const override = await getDefaultServerUrl();
      if (isHttpUrl(override)) setSignInServer(override);
    } catch {
      // No override: the build default below.
    }
  }
  return signInServer(signedIn);
}
