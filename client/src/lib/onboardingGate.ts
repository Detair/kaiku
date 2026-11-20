/**
 * When the first-run tour has to show. Lives apart from the wizard so
 * `App.tsx` can decide it without loading the wizard's chunk.
 */

import { authState, setupStatusKnown } from "@/stores/auth";
import {
  preferences,
  isInitialized as preferencesInitialized,
} from "@/stores/preferences";

/** Not before sign-in state, preferences and the setup state are known, never over the admin setup. */
export const shouldShowOnboarding = (): boolean =>
  authState.isInitialized &&
  preferencesInitialized() &&
  !preferences().onboarding_completed &&
  setupStatusKnown() &&
  !authState.setupRequired;
