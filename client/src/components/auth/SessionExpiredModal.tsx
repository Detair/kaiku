import { Component, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { useNavigate } from "@solidjs/router";
import { LogIn } from "lucide-solid";
import { t } from "@/i18n";
import { authState, setAuthState, logout } from "@/stores/auth";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

const REASON_TEXT = {
  expired: "connection.session.expired",
  revoked: "connection.session.revoked",
  banned: "connection.session.banned",
  reused: "connection.session.reused",
} as const;

const SessionExpiredModal: Component = () => {
  const navigate = useNavigate();

  // One sign-out per notice, however often Escape or the button fires.
  let leaving = false;
  const handleLogin = async () => {
    if (leaving) return;
    leaving = true;
    try {
      await logout();
    } catch {
      // Logout may fail if session is already gone
    }
    setAuthState({ sessionExpired: false });
    navigate("/login", { replace: true });
    leaving = false;
  };

  return (
    <Show when={authState.sessionExpired}>
      {(_shown) => {
      // Taken when the session ends, so it is in front of every window open
      // then and has the keyboard: focus on Log in, Tab stays here. Escape
      // means Log in, as it always did here, and never reaches the window below.
      let logIn: HTMLButtonElement | undefined;
      const dialog = createDialog({ onClose: () => void handleLogin(), initialFocus: () => logIn });
      const layer = createModalLayer(MODAL_LAYER.prompt);
      return (
      <Portal>
        <div class="fixed inset-0 flex items-center justify-center" style={{ "z-index": layer }}>
          <div class="absolute inset-0 bg-black/60 backdrop-blur-sm" />

          <div
            {...dialog.dialogProps}
            ref={dialog.ref}
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style={{"background-color":"var(--color-surface-layer1)"}}
          >
            <div class="flex items-center gap-3 px-5 py-4 border-b border-white/10">
              <div class="w-9 h-9 rounded-lg bg-status-warning/20 flex items-center justify-center">
                <LogIn class="w-5 h-5 text-status-warning" />
              </div>
              <h2 id={dialog.titleId} class="text-lg font-bold text-text-primary">
                {authState.sessionEndReason === "expired"
                  ? t("connection.session.expiredTitle")
                  : t("connection.session.endedTitle")}
              </h2>
            </div>

            <div class="p-5 space-y-4">
              <p class="text-text-secondary text-sm">
                {t(REASON_TEXT[authState.sessionEndReason])}
              </p>

              <div class="flex justify-end">
                <button
                  ref={logIn}
                  onClick={handleLogin}
                  class="px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90"
                >
                  {t("connection.session.logIn")}
                </button>
              </div>
            </div>
          </div>
        </div>
      </Portal>
      );
      }}
    </Show>
  );
};

export default SessionExpiredModal;
