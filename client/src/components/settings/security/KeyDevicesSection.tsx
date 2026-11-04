/**
 * Phones that are this account's key (C26 key devices): state, when a
 * pending one is ready, "Remove device" (step-up `key_device.remove`/id) and
 * "Set up a phone" (Task A7, full session).
 */

import { Component, For, JSX, Show, createSignal, onCleanup, onMount } from "solid-js";
import { AUTHENTICATORS_CHANGED_EVENT } from "@/lib/types/events";
import { Smartphone } from "lucide-solid";
import { totalKey } from "@/lib/totalKey";
import { t, type MessageKey } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import { listKeyDevices, removeKeyDevice } from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import type { KeyDevice } from "@/lib/types";
import { isProbation } from "@/stores/auth";
import { offerSignOutOthers, refreshMeSecurity } from "@/stores/security";
import SectionCard, { dangerButton, errorBox, errorStyle } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

const STATE_TEXT: Record<KeyDevice["state"], MessageKey> = {
  active: "security.keyDevices.state.active",
  pending: "security.keyDevices.state.pending",
  revoked: "security.keyDevices.state.revoked",
  unknown: "security.keyDevices.state.unknown",
};

export interface KeyDevicesSectionProps {
  /** "Set up a phone" (Task A7); hidden when not given. */
  setupPhone?: JSX.Element;
}

const KeyDevicesSection: Component<KeyDevicesSectionProps> = (props) => {
  const [devices, setDevices] = createSignal<KeyDevice[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal<string | null>(null);

  const load = async () => {
    try {
      setDevices((await listKeyDevices()).filter((d) => d.state !== "revoked"));
    } catch (err) {
      setError(securityErrorText(err, "security.keyDevices.loadFailed"));
    }
  };
  // C26: a device added, activated or removed elsewhere (or by "Set up a
  // phone" here) shows without reopening the settings (Argon review 3 #5).
  const onChanged = () => void load();
  onMount(() => {
    void load();
    window.addEventListener(AUTHENTICATORS_CHANGED_EVENT, onChanged);
  });
  onCleanup(() => window.removeEventListener(AUTHENTICATORS_CHANGED_EVENT, onChanged));

  const remove = async (d: KeyDevice) => {
    setBusy(d.id);
    setError(null);
    try {
      await withStepUp({ action: "key_device.remove", target: d.id, targetLabel: d.label }, () =>
        removeKeyDevice(d.id),
      );
      setDevices((list) => list.filter((x) => x.id !== d.id));
      void refreshMeSecurity();
      offerSignOutOthers();
    } catch (err) {
      setError(securityErrorText(err, "security.keyDevices.removeFailed"));
    } finally {
      setBusy(null);
    }
  };

  return (
    <SectionCard
      title={t("security.keyDevices.title")}
      icon={<Smartphone class="w-5 h-5 text-text-secondary" aria-hidden="true" />}
      testId="security-key-devices"
    >
      <Show when={devices().length > 0} fallback={<p class="text-base text-text-secondary">{t("security.keyDevices.none")}</p>}>
        <ul class="space-y-2">
          <For each={devices()}>
            {(d) => (
              <li class="flex flex-wrap items-center gap-3 p-3 rounded-lg bg-surface-layer2">
                <div class="flex-1 min-w-0">
                  <p class="text-base font-medium text-text-primary">{d.label}</p>
                  <p class="text-sm text-text-secondary">
                    {d.state === "pending" && d.ready_at
                      ? t("security.keyDevices.readyAt", { time: formatTimestamp(d.ready_at) })
                      : t(totalKey(STATE_TEXT, d.state, STATE_TEXT.unknown))}
                  </p>
                </div>
                <Show when={!isProbation()}>
                  <button
                    type="button"
                    class={dangerButton}
                    disabled={busy() === d.id}
                    aria-label={t("security.keyDevices.removeNamed", { name: d.label })}
                    onClick={() => void remove(d)}
                  >
                    {t("security.keyDevices.remove")}
                  </button>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>
          {error()}
        </p>
      </Show>
      <Show when={!isProbation() && props.setupPhone}>{props.setupPhone}</Show>
    </SectionCard>
  );
};

export default KeyDevicesSection;
