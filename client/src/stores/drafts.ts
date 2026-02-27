/**
 * Drafts Store
 *
 * Manages per-channel message drafts with localStorage persistence.
 * Features:
 * - Debounced save (300ms) with beforeunload flush
 * - LRU eviction at 50 drafts
 * - Skips E2EE channels (no plaintext leak)
 */

import { createSignal } from "solid-js";

// ============================================================================
// Constants
// ============================================================================

const STORAGE_KEY = "vc:drafts";
const DEBOUNCE_MS = 300;
const MAX_DRAFTS = 50;

// ============================================================================
// Types
// ============================================================================

interface DraftEntry {
  content: string;
  updatedAt: number;
}

type DraftsMap = Record<string, DraftEntry>;

// ============================================================================
// Signals
// ============================================================================

const [drafts, setDrafts] = createSignal<DraftsMap>({});

// ============================================================================
// Debounce Timer
// ============================================================================

let saveTimer: ReturnType<typeof setTimeout> | null = null;
let isDirty = false;

// ============================================================================
// localStorage Functions
// ============================================================================

/**
 * Load drafts from localStorage.
 */
function loadFromLocalStorage(): DraftsMap {
  if (typeof localStorage === "undefined") return {};

  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (e) {
    console.error("[Drafts] Failed to load from localStorage:", e);
  }
  return {};
}

/**
 * Save drafts to localStorage immediately.
 */
function saveToLocalStorage(data: DraftsMap): void {
  if (typeof localStorage === "undefined") return;

  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
    isDirty = false;
  } catch (e) {
    if (e instanceof DOMException && e.name === "QuotaExceededError") {
      console.warn(
        "[Drafts] localStorage quota exceeded, reducing to 25 drafts",
      );
      // Force evict to half the limit and retry
      const entries = Object.entries(data);
      entries.sort((a, b) => a[1].updatedAt - b[1].updatedAt);
      const reduced = entries.slice(-Math.floor(MAX_DRAFTS / 2));
      const reducedMap = Object.fromEntries(reduced);

      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(reducedMap));
        isDirty = false;
        // Update signal to reflect reduced drafts
        setDrafts(reducedMap);
      } catch (retryError) {
        console.error(
          "[Drafts] Failed to save even after reduction:",
          retryError,
        );
      }
    } else {
      console.error("[Drafts] Failed to save to localStorage:", e);
    }
  }
}

/**
 * Flush pending saves immediately.
 */
function flushSave(): void {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  if (isDirty) {
    saveToLocalStorage(drafts());
  }
}

/**
 * LRU eviction - remove oldest drafts when limit exceeded.
 */
function evictOldest(data: DraftsMap): DraftsMap {
  const entries = Object.entries(data);
  if (entries.length <= MAX_DRAFTS) return data;

  // Sort by updatedAt ascending (oldest first)
  entries.sort((a, b) => a[1].updatedAt - b[1].updatedAt);

  // Keep only the most recent MAX_DRAFTS entries
  const kept = entries.slice(-MAX_DRAFTS);
  return Object.fromEntries(kept);
}

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initialize drafts store on app start.
 */
export function initDrafts(): void {
  const loaded = loadFromLocalStorage();
  setDrafts(loaded);
  console.log(
    "[Drafts] Loaded",
    Object.keys(loaded).length,
    "drafts from localStorage",
  );

  // Register beforeunload handler for flush
  if (typeof window !== "undefined") {
    window.addEventListener("beforeunload", flushSave);
  }
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Get draft for a specific channel.
 */
export function getDraft(channelId: string): string {
  return drafts()[channelId]?.content ?? "";
}

/**
 * Save draft for a channel (debounced).
 * @param channelId - Channel ID
 * @param content - Draft content
 * @param isE2EE - Skip saving if true (no plaintext leak)
 */
export function saveDraft(
  channelId: string,
  content: string,
  isE2EE = false,
): void {
  // Skip E2EE channels to prevent plaintext leak
  if (isE2EE) return;

  const trimmed = content.trim();

  setDrafts((prev) => {
    let updated = { ...prev };

    if (!trimmed) {
      // Remove empty drafts
      delete updated[channelId];
    } else {
      // Update or add draft
      updated[channelId] = {
        content,
        updatedAt: Date.now(),
      };

      // Apply LRU eviction
      updated = evictOldest(updated);
    }

    return updated;
  });

  isDirty = true;

  // Debounced save
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveToLocalStorage(drafts());
    saveTimer = null;
  }, DEBOUNCE_MS);
}

/**
 * Clear draft for a specific channel.
 */
export function clearDraft(channelId: string): void {
  setDrafts((prev) => {
    const updated = { ...prev };
    delete updated[channelId];
    return updated;
  });

  isDirty = true;

  // Immediate save for clear (user sent message)
  flushSave();
}

/**
 * Clear all drafts (called on logout).
 */
export function clearAllDrafts(): void {
  setDrafts({});
  isDirty = true;
  flushSave();
  console.log("[Drafts] Cleared all drafts");
}

/**
 * Cleanup drafts store (called on logout).
 * Removes event listeners and clears timers.
 */
export function cleanupDrafts(): void {
  if (typeof window !== "undefined") {
    window.removeEventListener("beforeunload", flushSave);
  }
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
}

// ============================================================================
// Exports
// ============================================================================

export { drafts };
