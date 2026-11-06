/**
 * SecurityPromptBanner — one migration prompt at a time (C27 `prompts`, F8):
 * secure the account, set up a phone, add a passkey, sign in with one, name a
 * recovery contact, renew TOTP or recovery codes. "Later" snoozes 7 days.
 */

import { Component, Match, Show, Switch, createSignal, onCleanup } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { ShieldCheck } from "lucide-solid";
import { t, type MessageKey } from "@/i18n";
import type { SecurityPrompt } from "@/lib/types";
import { currentUser, isProbation, replaceSessionWithPasskey } from "@/stores/auth";
import { currentPrompt, snoozePrompt } from "@/stores/security";
import { requestSettings } from "@/stores/settingsNav";
import { canUsePasskeys } from "@/lib/tauri/passkeys";
import { passkeyFailureText } from "@/lib/authErrors";
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

const action = "btn-primary min-h-11 text-base";

const SecurityPromptBanner: Component = () => {
  const navigate = useNavigate();
  const [setupPhone, setSetupPhone] = createSignal(false);
  // In a chat-only session the chat-only banner already says where to add a
  // passkey: the same words in a second row help nobody.
  const prompt = () => {
    const current = currentPrompt(currentUser()?.id ?? "");
    return current === "add_passkey" && isProbation() ? null : current;
  };
  // "Sign in with a passkey": busy, worded failures, aborted with the banner
  // (Argon review 2 #5).
  const [signingIn, setSigningIn] = createSignal(false);
  const [signInError, setSignInError] = createSignal<string | null>(null);
  let attempt: AbortController | null = null;
  onCleanup(() => attempt?.abort());
  const signIn = async () => {
    attempt?.abort();
    const ctl = new AbortController();
    attempt = ctl;
    setSigningIn(true);
    setSignInError(null);
    try {
      await replaceSessionWithPasskey(ctl.signal);
    } catch (err) {
      if (!ctl.signal.aborted) setSignInError(passkeyFailureText(err, "auth.probation.signInFailed") || null);
    } finally {
      if (attempt === ctl) {
        attempt = null;
        setSigningIn(false);
      }
    }
  };

  return (
    <Show when={prompt()}>
      {(p) => (
        <div
          role="region"
          aria-label={t("security.prompt.label")}
          class="w-full shrink-0 flex flex-wrap items-center gap-x-4 gap-y-2 pl-3 pr-4 py-2.5 bg-[rgb(var(--color-surface-layer2-rgb)/0.96)] backdrop-blur-sm border-b border-white/10 border-l-4 border-l-accent-primary shadow-sm"
        >
          <div class="flex items-start gap-3 flex-1 min-w-64">
            <ShieldCheck class="w-5 h-5 mt-0.5 shrink-0 text-accent-primary" aria-hidden="true" />
            <p class="text-base text-text-primary">{t(TEXT[p()])}</p>
          </div>
          <div class="flex flex-wrap shrink-0 max-w-full items-center justify-end gap-2">
            <Switch>
              <Match when={p() === "secure_my_account"}>
                <button type="button" class={action} onClick={() => navigate("/secure-account")}>
                  {t("auth.login.secureAccount")}
                </button>
              </Match>
              <Match when={p() === "set_up_phone"}>
                <Show
                  when={!isProbation()}
                  fallback={<p class="text-sm text-text-primary">{t("security.prompt.askAdminPhone")}</p>}
                >
                  <button type="button" class={action} onClick={() => setSetupPhone(true)}>
                    {t("security.setupPhone.button")}
                  </button>
                </Show>
              </Match>
              <Match when={p() === "add_passkey"}>
                <Show
                  when={!isProbation()}
                  fallback={<p class="text-sm text-text-primary">{t("auth.probation.addPasskeyHint")}</p>}
                >
                  {/* LP's "Add a passkey" needs the user's click on it (WebAuthn), so the prompt opens the section. */}
                  <button type="button" class={action} onClick={() => requestSettings("security")}>
                    {t("security.prompt.openSettings")}
                  </button>
                </Show>
              </Match>
              <Match when={p() === "sign_in_with_passkey"}>
                <Show when={canUsePasskeys()}>
                  <button type="button" class={action} disabled={signingIn()} onClick={() => void signIn()}>
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
              class="min-h-11 px-3 rounded-lg text-base text-text-primary hover:bg-white/10"
              onClick={() => snoozePrompt(p(), currentUser()?.id ?? "")}
            >
              {t("security.prompt.later")}
            </button>
          </div>
          <Show when={signInError()}>
            <p role="alert" class="w-full text-sm text-text-primary">{signInError()}</p>
          </Show>
          <Show when={setupPhone()}>
            <SetupPhoneModal onClose={() => setSetupPhone(false)} />
          </Show>
        </div>
      )}
    </Show>
  );
};

export default SecurityPromptBanner;
