/**
 * "Browser for passkey sign-in" (desktop only, F1/DC14): which browser the
 * app opens for passkey ceremonies. Safari is offered on macOS only; on
 * Linux the help names the Firefox limit (B.6.8).
 */

import { Component, createResource, For, Show } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import { isTauri } from "@/lib/tauri/common";
import { getSettings, updateSettings } from "@/lib/tauri/settings";
import type { PasskeyBrowser } from "@/lib/types";

const CHOICES: { value: PasskeyBrowser; key: MessageKey; macOnly?: boolean }[] = [
  { value: "system", key: "passkeys.browser.system" },
  { value: "chromium", key: "passkeys.browser.chromium" },
  { value: "chrome", key: "passkeys.browser.chrome" },
  { value: "firefox", key: "passkeys.browser.firefox" },
  { value: "safari", key: "passkeys.browser.safari", macOnly: true },
];

const BrowserChoice: Component = () => {
  const ua = navigator.userAgent;
  const mac = /Mac OS X|Macintosh/.test(ua);
  const linux = /Linux/.test(ua) && !/Android/.test(ua);
  const [settings, { mutate }] = createResource(getSettings);

  const choose = async (value: PasskeyBrowser) => {
    const current = settings();
    if (!current) return;
    const next = { ...current, passkey_browser: value };
    mutate(next);
    await updateSettings(next);
  };

  return (
    <div class="space-y-2">
      <label for="passkey-browser" class="block text-sm font-medium text-text-primary">
        {t("passkeys.browser.title")}
      </label>
      <select
        id="passkey-browser"
        class="input-field min-h-12"
        value={settings()?.passkey_browser ?? "system"}
        disabled={!settings()}
        onChange={(e) => void choose(e.currentTarget.value as PasskeyBrowser)}
      >
        <For each={CHOICES.filter((c) => !c.macOnly || mac)}>
          {(c) => <option value={c.value}>{t(c.key)}</option>}
        </For>
      </select>
      <p class="text-sm text-text-secondary">{t("passkeys.browser.help")}</p>
      <Show when={linux}>
        <p class="text-sm text-text-secondary">{t("passkeys.browser.linuxHelp")}</p>
      </Show>
    </div>
  );
};

const PasskeyBrowserSetting: Component = () => (
  <Show when={isTauri()}>
    <BrowserChoice />
  </Show>
);

export default PasskeyBrowserSetting;
