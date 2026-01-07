/**
 * Authentication Store
 *
 * Manages user authentication state and actions.
 */

import { createStore } from "solid-js/store";
import { createSignal, createEffect } from "solid-js";
import type { User } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// Auth state interface
interface AuthState {
  user: User | null;
  serverUrl: string | null;
  isLoading: boolean;
  isInitialized: boolean;
  error: string | null;
}

// Create the store
const [authState, setAuthState] = createStore<AuthState>({
  user: null,
  serverUrl: null,
  isLoading: false,
  isInitialized: false,
  error: null,
});

// Derived state
export const isAuthenticated = () => authState.user !== null;
export const currentUser = () => authState.user;

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
    const user = await tauri.login(serverUrl, username, password);
    setAuthState({
      user,
      serverUrl,
      isLoading: false,
      error: null,
    });
    return user;
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
    const user = await tauri.register(
      serverUrl,
      username,
      password,
      email,
      displayName
    );
    setAuthState({
      user,
      serverUrl,
      isLoading: false,
      error: null,
    });
    return user;
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

// Export the store for reading
export { authState };
