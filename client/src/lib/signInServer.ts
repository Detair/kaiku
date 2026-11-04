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

/**
 * An http(s) server, never the desktop app's own origin (`tauri://localhost`,
 * `http://tauri.localhost` on Windows; Flash r3 p1 #3).
 */
function isHttpUrl(url: string | null | undefined): url is string {
  if (!url || !/^https?:\/\//i.test(url)) return false;
  let host: string;
  try {
    host = new URL(url).hostname;
  } catch {
    return false;
  }
  return host !== "tauri.localhost" && !(isTauri() && sameOrigin(url));
}

function sameOrigin(url: string): boolean {
  try {
    return typeof window !== "undefined" && new URL(url).origin === window.location.origin;
  } catch {
    return false;
  }
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

/** Forget the choice (tests; a new app start forgets it too). */
export function forgetSignInServer(): void {
  chosen = null;
  try {
    sessionStorage.removeItem(KEY);
  } catch {
    // Storage blocked: nothing was stored.
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

/**
 * The build-time default the sign-in page starts with; in a browser its own
 * http(s) origin, on the desktop nothing (`""`: the page asks for a server).
 */
function buildDefault(): string {
  const env = import.meta.env.VITE_SERVER_URL as string | undefined;
  if (isHttpUrl(env)) return baseOf(env);
  if (typeof window === "undefined") return "";
  const origin = window.location.origin;
  return isHttpUrl(origin) ? origin : "";
}

/**
 * The server for a pre-session flow now: the session's own server when
 * `signedIn`, else the sign-in page's choice, else the transport's or the
 * build default. Never the desktop app's own origin; `""` when nothing is
 * known (the pages then send nothing).
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
