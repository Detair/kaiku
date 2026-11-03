/**
 * LinuxPasskeyHint — LP's Linux/Firefox hint for a passkey ceremony in the
 * system browser (Firefox on Linux offers no phone as a passkey), with LP's
 * text key. Renders on Linux only; the caller shows it while the desktop app
 * waits for the browser.
 */

import { Component, Show } from "solid-js";
import { t } from "@/i18n";

/** Linux, not Android (the hint is about desktop Firefox). */
export function isLinuxDesktop(): boolean {
  return typeof navigator !== "undefined" && /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);
}

const LinuxPasskeyHint: Component<{ class?: string }> = (props) => (
  <Show when={isLinuxDesktop()}>
    <p class={props.class ?? "text-text-secondary"}>{t("passkeys.login.linuxHint")}</p>
  </Show>
);

export default LinuxPasskeyHint;
