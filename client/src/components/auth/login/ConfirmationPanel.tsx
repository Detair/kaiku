/**
 * ConfirmationPanel — the password or SSO sign-in waits for the phone (F3):
 * "Open Kaiku on your phone and scan this code." Nothing is sent to the phone.
 * "Phone not at hand?" continues the same attempt with TOTP (F3 step 5).
 */

import { Component, Show } from "solid-js";
import { t } from "@/i18n";
import type { LoginAttempt } from "@/lib/types";
import QrCodeImage from "../QrCodeImage";
import MinutesLeft from "./MinutesLeft";

export interface ConfirmationPanelProps {
  attempt: LoginAttempt;
  payload: string;
  via: "password" | "oidc";
  onPhoneNotAtHand: () => void;
  onBack: () => void;
}

const ConfirmationPanel: Component<ConfirmationPanelProps> = (props) => (
  <section class="space-y-5" aria-live="polite">
    <h2 class="text-xl font-semibold text-text-primary text-center">{t("auth.confirm.heading")}</h2>
    <p class="text-base text-text-primary text-center">{t("auth.confirm.scan")}</p>
    <div class="flex justify-center">
      <QrCodeImage payload={props.payload} alt={t("auth.confirm.qrAlt")} size={240} />
    </div>
    <MinutesLeft expiresAt={props.attempt.expires_at} />
    <Show when={props.attempt.totp_available}>
      <button
        type="button"
        class="w-full min-h-12 rounded-lg border border-white/10 text-base text-text-primary hover:bg-white/5"
        onClick={() => props.onPhoneNotAtHand()}
      >
        {t("auth.confirm.phoneNotAtHand")}
      </button>
    </Show>
    <button
      type="button"
      class="w-full min-h-12 rounded-lg text-base text-text-secondary hover:text-text-primary"
      onClick={() => props.onBack()}
    >
      {t("auth.flow.back")}
    </button>
  </section>
);

export default ConfirmationPanel;
