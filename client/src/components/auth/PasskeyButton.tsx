import { Component, Show } from "solid-js";
import { FingerprintPattern } from "lucide-solid";
import { t } from "@/i18n";

/** "Sign in with passkey": the first sign-in option (F1). */
const PasskeyButton: Component<{ busy: boolean; onClick: () => void }> = (props) => (
  <button
    type="button"
    class="btn-primary w-full min-h-12 flex items-center justify-center gap-2 text-base"
    onClick={() => props.onClick()}
    disabled={props.busy}
    aria-busy={props.busy ? "true" : "false"}
    data-testid="login-passkey"
  >
    <Show
      when={!props.busy}
      fallback={
        <>
          <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
          {t("passkeys.login.busy")}
        </>
      }
    >
      <FingerprintPattern class="w-5 h-5" aria-hidden="true" />
      {t("passkeys.login.button")}
    </Show>
  </button>
);

export default PasskeyButton;
