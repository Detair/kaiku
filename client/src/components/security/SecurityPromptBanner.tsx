/**
 * SecurityPromptBanner — one migration prompt at a time (C27 `prompts`, F8):
 * secure the account, set up a phone, add a passkey, sign in with one, name a
 * recovery contact, renew TOTP or recovery codes. "Later" snoozes 7 days.
 */

import { Component, Match, Show, Switch, createSignal } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { ShieldCheck } from "lucide-solid";
import { t, type MessageKey } from "@/i18n";
import type { SecurityPrompt } from "@/lib/types";
import { isProbation, replaceSessionWithPasskey } from "@/stores/auth";
import { currentPrompt, snoozePrompt } from "@/stores/security";
import { requestSettings } from "@/stores/settingsNav";
import { passkeyCeremonies } from "@/lib/passkeySeam";
import SetupPhoneModal from "@/components/settings/security/SetupPhoneModal";

const TEXT: Record<SecurityPrompt, MessageKey> = {
  secure_my_account: "security.prompt.secureMyAccount",
  set_up_phone: "security.prompt.setUpPhone",
  add_passkey: "security.prompt.addPasskey",
  sign_in_with_passkey: "security.prompt.signInWithPasskey",
  name_recovery_contact: "security.prompt.nameRecoveryContact",
  renew_totp: "security.prompt.renewTotp",
  renew_recovery_codes: "security.prompt.renewRecoveryCodes",
  open_request: "security.request.prompt",
};

const action = "btn-primary min-h-12 text-base";

const SecurityPromptBanner: Component = () => {
  const navigate = useNavigate();
  const [setupPhone, setSetupPhone] = createSignal(false);
  const prompt = () => currentPrompt();

  return (
    <Show when={prompt()}>
      {(p) => (
        <div
          role="region"
          aria-label={t("security.prompt.label")}
          class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[54] w-[min(40rem,calc(100vw-1rem))] rounded-xl border border-white/15 bg-surface-layer2 shadow-lg p-4 space-y-3"
        >
          <div class="flex items-start gap-3">
            <ShieldCheck class="w-5 h-5 mt-0.5 shrink-0 text-text-secondary" aria-hidden="true" />
            <p class="text-base text-text-primary">{t(TEXT[p()])}</p>
          </div>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Switch>
              <Match when={p() === "secure_my_account"}>
                <button type="button" class={action} onClick={() => navigate("/secure-account")}>
                  {t("auth.login.secureAccount")}
                </button>
              </Match>
              <Match when={p() === "set_up_phone"}>
                <Show
                  when={!isProbation()}
                  fallback={<p class="text-sm text-text-secondary mr-auto">{t("security.prompt.askAdminPhone")}</p>}
                >
                  <button type="button" class={action} onClick={() => setSetupPhone(true)}>
                    {t("security.setupPhone.button")}
                  </button>
                </Show>
              </Match>
              <Match when={p() === "add_passkey"}>
                <Show
                  when={!isProbation()}
                  fallback={<p class="text-sm text-text-secondary mr-auto">{t("auth.probation.addPasskeyHint")}</p>}
                >
                  {/* TODO(LP rebase): open LP's PasskeysSection "Add" directly. */}
                  <button type="button" class={action} onClick={() => requestSettings("security")}>
                    {t("security.prompt.openSettings")}
                  </button>
                </Show>
              </Match>
              <Match when={p() === "sign_in_with_passkey"}>
                <Show when={passkeyCeremonies()}>
                  <button type="button" class={action} onClick={() => void replaceSessionWithPasskey().catch(() => undefined)}>
                    {t("auth.probation.signIn")}
                  </button>
                </Show>
              </Match>
              <Match when={true}>
                <button type="button" class={action} onClick={() => requestSettings("security")}>
                  {t("security.prompt.openSettings")}
                </button>
              </Match>
            </Switch>
            <button
              type="button"
              class="min-h-12 px-4 rounded-lg text-base text-text-secondary hover:text-text-primary hover:bg-white/5"
              onClick={() => snoozePrompt(p())}
            >
              {t("security.prompt.later")}
            </button>
          </div>
          <Show when={setupPhone()}>
            <SetupPhoneModal onClose={() => setSetupPhone(false)} />
          </Show>
        </div>
      )}
    </Show>
  );
};

export default SecurityPromptBanner;
