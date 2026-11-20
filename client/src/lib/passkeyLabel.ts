/**
 * The default name of a new passkey made in this browser, e.g. "Firefox on
 * Linux" (renamed later in the security settings).
 */

import { t } from "@/i18n";

export function defaultPasskeyLabel(): string {
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Safari\//.test(ua)
          ? "Safari"
          : "Browser";
  const os = /Android/.test(ua)
    ? "Android"
    : /Windows/.test(ua)
      ? "Windows"
      : /Mac OS X|Macintosh/.test(ua)
        ? "macOS"
        : /Linux/.test(ua)
          ? "Linux"
          : "";
  return os ? t("passkeys.settings.defaultLabel", { browser, os }) : browser;
}
