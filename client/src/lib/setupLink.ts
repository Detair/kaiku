/**
 * Setup links (C27): `{PUBLIC_URL}/setup#v=1&t=<b64u setup token>`. The token
 * lives in the fragment, so neither the server nor a link preview receives
 * it; the page reads it once and removes it from the address bar.
 */

export type SetupFragment = { v: 1; token: string } | { error: "version" | "missing" };

/** A 256-bit token is 43 unpadded base64url characters; accept 32..128. */
const TOKEN_RE = /^[A-Za-z0-9_-]{32,128}$/;

export function parseSetupFragment(hash: string): SetupFragment {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const v = params.get("v");
  const token = params.get("t");
  if (v !== null && v !== "1") return { error: "version" };
  if (v === null || !token || !TOKEN_RE.test(token)) return { error: "missing" };
  return { v: 1, token };
}

/** The Kaiku Android app (`applicationId`); the hand-off goes to it and to no other app. */
const ANDROID_PACKAGE = "io.wolftown.kaiku";

/**
 * "Open in Kaiku" on Android, for the app's `kaiku://setup/v1` intent filter. The intent names
 * the Kaiku app's package, so the browser hands the code only to Kaiku (or, without the app,
 * offers to install it) and never to another app that registered the same scheme.
 */
export function androidIntentUrl(origin: string, token: string): string {
  return `intent://setup/v1?o=${encodeURIComponent(origin)}&t=${token}#Intent;scheme=kaiku;package=${ANDROID_PACKAGE};end`;
}

export function isAndroidBrowser(ua: string): boolean {
  return /\bAndroid\b/i.test(ua);
}
