/**
 * QrLoginPanel — "Sign in with your phone" (F2). Shows the attempt's QR code
 * (rotated by the poll, same attempt) and three numbered steps. There is no
 * typed sign-in code (F2); a phone that cannot scan uses a passkey or TOTP.
 */

import { Component, Show } from "solid-js";
import { t } from "@/i18n";
import type { LoginAttempt } from "@/lib/types";
import QrCodeImage from "../QrCodeImage";
import MinutesLeft from "./MinutesLeft";

export interface QrLoginPanelProps {
  attempt: LoginAttempt;
  payload: string;
  onBack: () => void;
  /**
   * "Use a passkey instead". TODO(LP rebase): Login passes LP's passkey
   * sign-in (`PasskeyButton` / `loginWithPasskey`); hidden until then.
   */
  onUsePasskey?: () => void;
}

const QrLoginPanel: Component<QrLoginPanelProps> = (props) => (
  <section class="space-y-5" aria-live="polite">
    <h2 class="text-xl font-semibold text-text-primary text-center">{t("auth.phone.heading")}</h2>
    <ol class="space-y-2 text-base text-text-primary list-decimal list-inside">
      <li>{t("auth.phone.step1")}</li>
      <li>{t("auth.phone.step2")}</li>
      <li>{t("auth.phone.step3")}</li>
    </ol>
    <div class="flex justify-center">
      <QrCodeImage payload={props.payload} alt={t("auth.phone.qrAlt")} size={240} />
    </div>
    <MinutesLeft expiresAt={props.attempt.expires_at} />
    <Show when={props.onUsePasskey}>
      {(usePasskey) => (
        <button type="button" class="btn-primary w-full min-h-12 text-base" onClick={() => usePasskey()()}>
          {t("auth.phone.usePasskey")}
        </button>
      )}
    </Show>
    <button
      type="button"
      class="w-full min-h-12 rounded-lg border border-white/10 text-base text-text-primary hover:bg-white/5"
      onClick={() => props.onBack()}
    >
      {t("auth.flow.back")}
    </button>
  </section>
);

export default QrLoginPanel;
