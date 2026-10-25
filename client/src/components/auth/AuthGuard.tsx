import { Component, JSX, Show, createEffect, onMount } from "solid-js";
import { useNavigate, useLocation } from "@solidjs/router";
import {
  abandonRestore,
  authState,
  initAuth,
  isAuthenticated,
  retryRestoreNow,
} from "@/stores/auth";
import { t } from "@/i18n";
import ForcedPasswordChange from "@/views/ForcedPasswordChange";

interface AuthGuardProps {
  children: JSX.Element;
}

/**
 * AuthGuard component that protects routes requiring authentication.
 *
 * If user is not authenticated, redirects to login page.
 * Shows loading state while checking authentication.
 */
const AuthGuard: Component<AuthGuardProps> = (props) => {
  const navigate = useNavigate();
  const location = useLocation();

  // Initialize auth on mount
  onMount(() => {
    initAuth();
  });

  // Redirect to login if not authenticated
  createEffect(() => {
    if (authState.isInitialized && !isAuthenticated()) {
      // Store the intended destination for redirect after login
      const returnUrl = location.pathname;
      navigate(`/login${returnUrl !== "/" ? `?returnUrl=${encodeURIComponent(returnUrl)}` : ""}`, {
        replace: true,
      });
    }
  });

  return (
    <Show
      when={authState.isInitialized}
      fallback={
        <Show when={authState.restorePending} fallback={<LoadingScreen />}>
          <RestorePendingScreen />
        </Show>
      }
    >
      <Show when={isAuthenticated()}>
        {/* C4: nothing else works until the temporary password is replaced. */}
        <Show when={!authState.passwordChangeRequired} fallback={<ForcedPasswordChange />}>
          {props.children}
        </Show>
      </Show>
    </Show>
  );
};

/**
 * Full-screen loading indicator shown while checking auth.
 */
const LoadingScreen: Component = () => {
  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary">
      <div class="flex flex-col items-center gap-4">
        <div class="w-12 h-12 border-4 border-primary/30 border-t-primary rounded-full animate-spin" />
        <p class="text-text-secondary">{t("common.loading")}</p>
      </div>
    </div>
  );
};

/**
 * A stored login exists but the server can't be reached yet (offline or slow
 * start). The login is kept and retried in the background.
 */
const RestorePendingScreen: Component = () => {
  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary px-4">
      <div
        class="flex flex-col items-center gap-4 max-w-sm text-center"
        role="status"
        aria-live="polite"
      >
        <div class="w-12 h-12 border-4 border-primary/30 border-t-primary rounded-full animate-spin" />
        <h1 class="text-lg font-semibold text-text-primary">
          {t("auth.restore.unreachableTitle")}
        </h1>
        <p class="text-text-secondary">{t("auth.restore.unreachable")}</p>
        <div class="flex gap-3">
          <button
            type="button"
            onClick={() => retryRestoreNow()}
            class="px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium transition-colors hover:bg-accent-primary/90"
          >
            {t("common.tryAgain")}
          </button>
          <button
            type="button"
            onClick={() => abandonRestore()}
            class="px-4 py-2 rounded-lg text-text-secondary hover:text-text-primary transition-colors"
          >
            {t("auth.restore.signInInstead")}
          </button>
        </div>
      </div>
    </div>
  );
};

export default AuthGuard;
