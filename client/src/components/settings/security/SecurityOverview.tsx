/**
 * Overview of how this account signs in (C27 `GET /api/me/security`):
 * password and TOTP state, recovery codes left, phones and passkeys, the
 * recovery contact. Severity is shown by icon and text, never colour alone.
 */

import { Component, For, Show, onMount } from "solid-js";
import { AlertTriangle, CheckCircle2, Shield } from "lucide-solid";
import { t, type MessageKey } from "@/i18n";
import type { CredentialState } from "@/lib/types";
import { meSecurity, refreshMeSecurity } from "@/stores/security";

const STATE: Record<CredentialState, MessageKey> = {
  active: "security.overview.state.active",
  throttled: "security.overview.state.throttled",
  compromised: "security.overview.state.compromised",
  unknown: "security.overview.state.unknown",
};

const SecurityOverview: Component = () => {
  onMount(() => void refreshMeSecurity());
  const rows = () => {
    const s = meSecurity();
    if (!s) return [];
    const contact = s.recovery_contact;
    return [
      {
        label: t("security.overview.password"),
        value: s.password ? t(STATE[s.password.state]) : t("security.overview.none"),
        warn: s.password?.state === "compromised" || s.password?.state === "throttled",
      },
      {
        label: t("security.overview.totp"),
        value: s.totp ? t(STATE[s.totp.state]) : t("security.overview.none"),
        warn: s.totp?.state === "compromised" || s.totp?.state === "throttled",
      },
      { label: t("security.overview.recoveryCodes"), value: String(s.recovery_codes_left), warn: s.recovery_codes_left === 0 },
      { label: t("security.overview.phones"), value: String(s.key_devices), warn: false },
      { label: t("security.overview.passkeys"), value: String(s.passkeys), warn: false },
      {
        label: t("security.overview.contact"),
        value: contact
          ? contact.confirmed_at
            ? t("security.contact.confirmed")
            : t("security.contact.waiting")
          : t("security.overview.none"),
        warn: false,
      },
    ];
  };
  return (
    <section class="bg-surface-base rounded-xl p-4 space-y-3" data-testid="security-overview">
      <div class="flex items-center gap-3">
        <Shield class="w-5 h-5 text-text-secondary" aria-hidden="true" />
        <h4 class="text-base font-semibold text-text-primary">{t("security.overview.title")}</h4>
      </div>
      <Show when={meSecurity()} fallback={<p class="text-base text-text-secondary">{t("common.loading")}</p>}>
        <dl class="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <For each={rows()}>
            {(row) => (
              <div class="flex items-center justify-between gap-3 p-3 rounded-lg bg-surface-layer2">
                <dt class="text-base text-text-secondary">{row.label}</dt>
                <dd class="flex items-center gap-2 text-base text-text-primary">
                  <Show
                    when={row.warn}
                    fallback={<CheckCircle2 class="w-4 h-4 text-status-success" aria-hidden="true" />}
                  >
                    <AlertTriangle class="w-4 h-4 text-status-warning" aria-hidden="true" />
                  </Show>
                  {row.value}
                </dd>
              </div>
            )}
          </For>
        </dl>
        <Show when={meSecurity()?.password?.state === "compromised"}>
          <p class="text-base text-text-primary" role="status">{t("security.password.compromised")}</p>
        </Show>
      </Show>
    </section>
  );
};

export default SecurityOverview;
