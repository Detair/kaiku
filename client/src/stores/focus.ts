/**
 * Focus Store
 *
 * Manages focus mode state and notification policy evaluation.
 * All logic is client-side — focus configuration lives in UserPreferences JSONB,
 * runtime state (active mode) is device-local.
 */

import { createSignal } from "solid-js";
import type {
  FocusMode,
  FocusState,
  FocusTriggerCategory,
} from "@/lib/types";
import type { SoundEvent } from "@/lib/sound/types";
import { isDndActive } from "@/stores/sound";
import { preferences } from "./preferences";

// ============================================================================
// Runtime State (device-local, not persisted)
// ============================================================================

const [focusState, setFocusState] = createSignal<FocusState>({
  activeModeId: null,
  autoActivated: false,
  activatedAt: null,
  triggeringCategory: null,
});

// ============================================================================
// VIP Lookup Helpers
// ============================================================================

const EMPTY_SET: ReadonlySet<string> = new Set();

/**
 * Build a Set from a mode's VIP list for O(1) lookups.
 * Called on every evaluateFocusPolicy to stay in sync with live preferences.
 * Lists are capped at 50 entries so construction cost is negligible.
 */
function buildVipSet(ids: string[]): ReadonlySet<string> {
  return ids.length > 0 ? new Set(ids) : EMPTY_SET;
}

// ============================================================================
// Mode Accessors
// ============================================================================

/**
 * Get the currently active focus mode, or null if none.
 */
export function getActiveFocusMode(): FocusMode | null {
  const state = focusState();
  if (!state.activeModeId) return null;

  const modes = preferences().focus?.modes;
  if (!modes) return null;

  return modes.find((m) => m.id === state.activeModeId) ?? null;
}

// ============================================================================
// Activation / Deactivation
// ============================================================================

/**
 * Manually activate a focus mode by ID.
 */
export function activateFocusMode(modeId: string): void {
  const modes = preferences().focus?.modes;
  if (!modes) return;

  const mode = modes.find((m) => m.id === modeId);
  if (!mode) return;

  setFocusState({
    activeModeId: modeId,
    autoActivated: false,
    activatedAt: new Date().toISOString(),
    triggeringCategory: null,
  });
}

/**
 * Deactivate the current focus mode.
 */
export function deactivateFocusMode(): void {
  setFocusState({
    activeModeId: null,
    autoActivated: false,
    activatedAt: null,
    triggeringCategory: null,
  });
}

/**
 * Handle activity category changes from the process scanner.
 * Auto-activates/deactivates focus modes when the global toggle and
 * per-mode toggle are both enabled.
 */
export function handleActivityChange(
  category: FocusTriggerCategory | null
): void {
  const state = focusState();
  const focusPrefs = preferences().focus;
  if (!focusPrefs) return;

  // If activity cleared and current mode was auto-activated, deactivate
  // (must run even when autoActivateGlobal is off — user may have toggled it
  // off while an auto-activated mode was running)
  if (category === null) {
    if (state.autoActivated) {
      deactivateFocusMode();
    }
    return;
  }

  // Master switch must be on for new activations
  if (!focusPrefs.autoActivateGlobal) return;

  // Don't override a manually activated mode
  if (state.activeModeId && !state.autoActivated) return;

  // Find a mode that matches this category and has auto-activate enabled
  const matchingMode = focusPrefs.modes.find(
    (m) =>
      m.autoActivateEnabled &&
      m.triggerCategories !== null &&
      m.triggerCategories.includes(category)
  );

  if (matchingMode) {
    // Already active for this mode? Skip
    if (state.activeModeId === matchingMode.id) return;

    setFocusState({
      activeModeId: matchingMode.id,
      autoActivated: true,
      activatedAt: new Date().toISOString(),
      triggeringCategory: category,
    });
  } else if (state.autoActivated) {
    // Category changed but no mode matches — deactivate if was auto-activated
    deactivateFocusMode();
  }
}

// ============================================================================
// Notification Policy Evaluation
// ============================================================================

/**
 * Evaluate whether a notification event should be suppressed or allowed
 * based on the current focus state.
 *
 * Priority:
 * 1. DND/quiet hours active → suppress (absolute, no overrides)
 * 2. No focus mode → allow
 * 3. VIP user → allow
 * 4. VIP channel → allow
 * 5. Emergency keyword match → allow
 * 6. Apply suppression level
 */
export function evaluateFocusPolicy(
  event: SoundEvent
): "suppress" | "allow" {
  // 1. DND is absolute — no overrides
  if (isDndActive()) {
    return "suppress";
  }

  // 2. No active focus mode → allow (fall through to existing checks)
  const mode = getActiveFocusMode();
  if (!mode) {
    return "allow";
  }

  // 3. VIP user check (O(1) Set lookup, rebuilt from live preferences)
  if (event.authorId && buildVipSet(mode.vipUserIds).has(event.authorId)) {
    return "allow";
  }

  // 4. VIP channel check (O(1) Set lookup, rebuilt from live preferences)
  if (buildVipSet(mode.vipChannelIds).has(event.channelId)) {
    return "allow";
  }

  // 5. Emergency keyword check (linear scan, max 5 keywords)
  if (
    event.content &&
    mode.emergencyKeywords.length > 0
  ) {
    const lowerContent = event.content.toLowerCase();
    for (const keyword of mode.emergencyKeywords) {
      if (lowerContent.includes(keyword.toLowerCase())) {
        return "allow";
      }
    }
  }

  // 6. Apply suppression level
  switch (mode.suppressionLevel) {
    case "all":
      return "suppress";

    case "except_mentions":
      return event.mentionType ? "allow" : "suppress";

    case "except_dms":
      return event.isDm ? "allow" : "suppress";

    default:
      return "suppress";
  }
}

// ============================================================================
// Exports
// ============================================================================

export { focusState };
