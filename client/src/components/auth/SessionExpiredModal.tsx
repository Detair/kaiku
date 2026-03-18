import { Component, Show, onMount, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { useNavigate } from "@solidjs/router";
import { LogIn } from "lucide-solid";
import { authState, setAuthState, logout } from "@/stores/auth";

const SessionExpiredModal: Component = () => {
  const navigate = useNavigate();

  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && authState.sessionExpired) handleLogin();
    };
    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  const handleLogin = async () => {
    try {
      await logout();
    } catch {
      // Logout may fail if session is already gone
    }
    setAuthState({ sessionExpired: false });
    navigate("/login", { replace: true });
  };

  return (
    <Show when={authState.sessionExpired}>
      <Portal>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          <div class="absolute inset-0 bg-black/60 backdrop-blur-sm" />

          <div
            class="relative rounded-xl border border-white/10 w-[400px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
            style="background-color: var(--color-surface-layer1)"
          >
            <div class="flex items-center gap-3 px-5 py-4 border-b border-white/10">
              <div class="w-9 h-9 rounded-lg bg-status-warning/20 flex items-center justify-center">
                <LogIn class="w-5 h-5 text-status-warning" />
              </div>
              <h2 class="text-lg font-bold text-text-primary">
                Session Expired
              </h2>
            </div>

            <div class="p-5 space-y-4">
              <p class="text-text-secondary text-sm">
                Your session has expired. Please log in again to continue.
              </p>

              <div class="flex justify-end">
                <button
                  onClick={handleLogin}
                  class="px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90"
                >
                  Log in
                </button>
              </div>
            </div>
          </div>
        </div>
      </Portal>
    </Show>
  );
};

export default SessionExpiredModal;
