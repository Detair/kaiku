/**
 * ProbationBanner — a chat-only session (B.4) says what it cannot do and how
 * to get a full session: "Sign in with a passkey" when the account has one,
 * otherwise where to add one. It collapses to a labelled chip for this run of
 * the app and is never hidden for good.
 */

import { Component, Show, createEffect, createSignal } from "solid-js";
import { ShieldAlert } from "lucide-solid";
import { t } from "@/i18n";
import { isProbation, replaceSessionWithPasskey } from "@/stores/auth";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import { canUsePasskeys } from "@/lib/tauri/passkeys";
import { passkeyFailureText } from "@/lib/authErrors";

// Per chat-only session, not stored: the banner comes back after a restart
// and for every new chat-only session.
const [collapsed, setCollapsed] = createSignal(false);

const ProbationBanner: Component = () => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  // The banner stays mounted while sessions change: whenever this turns into
  // a chat-only session, load the overview (passkey button or hint); a full
  // session in between opens the banner again for the next one.
  createEffect(() => {
    if (isProbation()) void refreshMeSecurity();
    else setCollapsed(false);
  });
  const hasPasskey = () => (meSecurity()?.passkeys ?? 0) > 0;
  const canSignIn = () => hasPasskey() && canUsePasskeys();

  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      await replaceSessionWithPasskey();
    } catch (err) {
      setError(passkeyFailureText(err, "auth.probation.signInFailed") || null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={isProbation()}>
      <Show
        when={!collapsed()}
        fallback={
          <button
            type="button"
            class="fixed top-2 left-1/2 -translate-x-1/2 z-[55] flex items-center gap-2 px-4 min-h-11 rounded-full border border-status-warning/60 bg-surface-layer2 text-base text-text-primary shadow"
            onClick={() => setCollapsed(false)}
            aria-expanded="false"
          >
            <ShieldAlert class="w-5 h-5 text-status-warning" aria-hidden="true" />
            {t("auth.probation.chip")}
          </button>
        }
      >
        <div
          role="region"
          aria-label={t("auth.probation.chip")}
          class="fixed top-2 left-1/2 -translate-x-1/2 z-[55] w-[min(40rem,calc(100vw-1rem))] rounded-xl border border-status-warning/60 bg-surface-layer2 shadow-lg p-4 space-y-3"
        >
          <div class="flex items-start gap-3">
            <ShieldAlert class="w-5 h-5 mt-0.5 text-status-warning shrink-0" aria-hidden="true" />
            <p class="text-base text-text-primary">{t("auth.probation.text")}</p>
          </div>
          <Show when={error()}>
            <p role="alert" class="text-sm text-text-primary">{error()}</p>
          </Show>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Show
              when={canSignIn()}
              fallback={
                <Show when={meSecurity() && !hasPasskey()}>
                  <p class="text-sm text-text-secondary mr-auto">{t("auth.probation.addPasskeyHint")}</p>
                </Show>
              }
            >
              <button type="button" class="btn-primary min-h-12 text-base" disabled={busy()} onClick={() => void signIn()}>
                {t("auth.probation.signIn")}
              </button>
            </Show>
            <button
              type="button"
              class="min-h-12 px-4 rounded-lg text-base text-text-secondary hover:text-text-primary hover:bg-white/5"
              onClick={() => setCollapsed(true)}
              aria-expanded="true"
            >
              {t("auth.probation.hide")}
            </button>
          </div>
        </div>
      </Show>
    </Show>
  );
};

export default ProbationBanner;
