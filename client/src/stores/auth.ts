/**
 * Authentication Store
 *
 * Manages user authentication state and actions.
 */

import { createStore } from "solid-js/store";
import type { User } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { initWebSocket, connect as wsConnect, disconnect as wsDisconnect, cleanupWebSocket } from "./websocket";
import { initPresence, cleanupPresence, initIdleDetection, stopIdleDetectionCleanup } from "./presence";
import { initPreferences } from "./preferences";
import { clearAllDrafts, cleanupDrafts } from "./drafts";

// Auth state interface
interface AuthState {
  user: User | null;
  serverUrl: string | null;
  isLoading: boolean;
  isInitialized: boolean;
  error: string | null;
  setupRequired: boolean;
}

// Create the store
const [authState, setAuthState] = createStore<AuthState>({
  user: null,
  serverUrl: null,
  isLoading: false,
  isInitialized: false,
  error: null,
  setupRequired: false,
});

// Derived state
export const isAuthenticated = () => authState.user !== null;
export const currentUser = () => authState.user;

// WebSocket reconnection listener (registered once globally to prevent leaks)
let wsReconnectListenerRegistered = false;

function registerWebSocketReconnectListener() {
  if (typeof window !== "undefined" && !wsReconnectListenerRegistered) {
    try {
      const handler = () => {
        console.log("[Auth] WebSocket reconnected, clearing error message");
        setAuthState("error", null);
      };
      window.addEventListener("ws-reconnected", handler);
      wsReconnectListenerRegistered = true;
      console.log("[Auth] WebSocket reconnection listener registered");
    } catch (e) {
      console.error("[Auth] Failed to register WebSocket reconnect listener:", e);
      // Non-critical, continue
    }
  }
}

// Actions

/**
 * Initialize auth state by checking for existing session.
 */
export async function initAuth(): Promise<void> {
  if (authState.isInitialized) return;

  setAuthState({ isLoading: true, error: null });

  try {
    const user = await tauri.getCurrentUser();
    setAuthState({
      user,
      isLoading: false,
      isInitialized: true,
    });

    // If user is restored, also reconnect WebSocket and init preferences
    if (user) {
      await initWebSocket();
      await initPresence();

      // Register WebSocket reconnection listener (once globally)
      registerWebSocketReconnectListener();

      try {
        await wsConnect();
        console.log("[Auth] WebSocket reconnected after session restore");
      } catch (wsErr) {
        console.error("[Auth] WebSocket reconnection failed:", wsErr);
        // WebSocket failure is critical for real-time messaging
        // The WebSocket module will auto-retry, but user should know there's an issue
        setAuthState("error", "Real-time messaging temporarily unavailable. Reconnecting...");
      }
      // Initialize preferences sync after session restore
      try {
        await initPreferences();
        console.log("[Auth] Preferences initialized after session restore");
      } catch (prefErr) {
        console.error("[Auth] Preferences initialization failed:", prefErr);
        // Continue even if preferences fail - non-critical
      }

      // Initialize idle detection after preferences (uses idleTimeoutMinutes setting)
      initIdleDetection();
    }
  } catch (err) {
    console.error("Failed to restore session:", err);
    setAuthState({
      user: null,
      isLoading: false,
      isInitialized: true,
      error: null, // Don't show error for session restoration
    });
  }
}

/**
 * Login with username and password.
 */
export async function login(
  serverUrl: string,
  username: string,
  password: string
): Promise<User> {
  setAuthState({ isLoading: true, error: null });

  try {
    const result = await tauri.login(serverUrl, username, password);
    setAuthState({
      user: result.user,
      serverUrl,
      isLoading: false,
      error: null,
      setupRequired: result.setup_required,
    });

    // Initialize WebSocket and presence after login
    await initWebSocket();
    await initPresence();

    // Register WebSocket reconnection listener (once globally)
    registerWebSocketReconnectListener();

    try {
      await wsConnect();
    } catch (wsErr) {
      console.error("WebSocket connection failed:", wsErr);
      // WebSocket failure is critical for real-time messaging
      // The WebSocket module will auto-retry, but user should know there's an issue
      setAuthState({ error: "Real-time messaging temporarily unavailable. Reconnecting..." });
      // Continue - user is still logged in and WebSocket will auto-retry
    }

    // Initialize preferences sync after login
    try {
      await initPreferences();
      console.log("[Auth] Preferences initialized after login");
    } catch (prefErr) {
      console.error("[Auth] Preferences initialization failed:", prefErr);
      // Continue even if preferences fail - non-critical
    }

    // Initialize idle detection after preferences (uses idleTimeoutMinutes setting)
    initIdleDetection();

    return result.user;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({ isLoading: false, error });
    throw new Error(error);
  }
}

/**
 * Register a new account.
 */
export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string
): Promise<User> {
  setAuthState({ isLoading: true, error: null });

  try {
    const result = await tauri.register(
      serverUrl,
      username,
      password,
      email,
      displayName
    );
    setAuthState({
      user: result.user,
      serverUrl,
      isLoading: false,
      error: null,
      setupRequired: result.setup_required,
    });

    // Initialize WebSocket and presence after registration
    await initWebSocket();
    await initPresence();
    try {
      await wsConnect();
    } catch (wsErr) {
      console.error("WebSocket connection failed:", wsErr);
    }

    // Initialize preferences sync after registration
    try {
      await initPreferences();
      console.log("[Auth] Preferences initialized after registration");
    } catch (prefErr) {
      console.error("[Auth] Preferences initialization failed:", prefErr);
      // Continue even if preferences fail - non-critical
    }

    // Initialize idle detection after preferences (uses idleTimeoutMinutes setting)
    initIdleDetection();

    return result.user;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({ isLoading: false, error });
    throw new Error(error);
  }
}

/**
 * Complete OIDC login after receiving tokens from the callback.
 * This is called after the OIDC provider redirects back with tokens.
 */
export async function loginWithOidc(
  serverUrl: string,
  accessToken: string,
  refreshToken: string,
  expiresIn: number
): Promise<void> {
  setAuthState({ isLoading: true, error: null });

  try {
    // Store tokens
    await tauri.oidcCompleteLogin(serverUrl, accessToken, refreshToken, expiresIn);

    // Fetch the current user with the new token
    const user = await tauri.getCurrentUser();
    if (!user) {
      throw new Error("Failed to fetch user after OIDC login");
    }

    setAuthState({
      user,
      serverUrl,
      isLoading: false,
      error: null,
      setupRequired: false,
    });

    // Initialize WebSocket and presence after OIDC login
    await initWebSocket();
    await initPresence();
    registerWebSocketReconnectListener();

    try {
      await wsConnect();
    } catch (wsErr) {
      console.error("WebSocket connection failed:", wsErr);
      setAuthState({ error: "Real-time messaging temporarily unavailable. Reconnecting..." });
    }

    try {
      await initPreferences();
    } catch (prefErr) {
      console.error("[Auth] Preferences initialization failed:", prefErr);
    }

    initIdleDetection();
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({ isLoading: false, error });
    throw new Error(error);
  }
}

/**
 * Logout and clear session.
 */
export async function logout(): Promise<void> {
  setAuthState({ isLoading: true, error: null });

  // Disconnect WebSocket and cleanup
  try {
    await wsDisconnect();
    await cleanupWebSocket();
    stopIdleDetectionCleanup();
    cleanupPresence();
    clearAllDrafts();
    cleanupDrafts();
  } catch (err) {
    console.error("Error during cleanup:", err);
  }

  try {
    await tauri.logout();
    setAuthState({
      user: null,
      isLoading: false,
      error: null,
    });
  } catch (err) {
    // Still clear local state even if server logout fails
    setAuthState({
      user: null,
      isLoading: false,
      error: null,
    });
  }
}

/**
 * Clear any auth errors.
 */
export function clearError(): void {
  setAuthState({ error: null });
}

/**
 * Update local user state.
 */
export function updateUser(updates: Partial<User>): void {
  setAuthState("user", (prev) => (prev ? { ...prev, ...updates } : null));
}

/**
 * Clear the setup required flag.
 * Called after completing the setup wizard.
 */
export function clearSetupRequired(): void {
  setAuthState("setupRequired", false);
}

// Export the store for reading only
// Use the exported functions (login, register, logout, etc.) to modify state
export { authState };
