import { Component, JSX, Show, createEffect, onMount } from "solid-js";
import { useNavigate, useLocation } from "@solidjs/router";
import { authState, initAuth, isAuthenticated } from "@/stores/auth";

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
      navigate(`/login${returnUrl !== "/" ? `?returnUrl=${returnUrl}` : ""}`, {
        replace: true,
      });
    }
  });

  return (
    <Show when={authState.isInitialized} fallback={<LoadingScreen />}>
      <Show when={isAuthenticated()}>{props.children}</Show>
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
        <p class="text-text-secondary">Loading...</p>
      </div>
    </div>
  );
};

export default AuthGuard;
