/**
 * Voice Adapter Factory
 *
 * Creates the appropriate voice adapter based on runtime environment
 */

import type { VoiceAdapter } from "./types";
import { isTauri } from "@/lib/tauri/common";

// Singleton instance
let adapterInstance: VoiceAdapter | null = null;
/** The adapter being created (its code is loaded on first use). */
let creating: Promise<VoiceAdapter> | null = null;

/**
 * Create or return the existing voice adapter. The adapter's code is not
 * part of the start-up bundle: it is loaded the first time voice is used.
 */
export function createVoiceAdapter(): Promise<VoiceAdapter> {
  if (adapterInstance) return Promise.resolve(adapterInstance);
  if (creating) return creating;
  const attempt = (async () => {
    console.log(
      `[VoiceAdapter] Creating ${isTauri() ? "Tauri" : "Browser"} adapter`,
    );
    const adapter: VoiceAdapter = isTauri()
      ? new (await import("./tauri")).TauriVoiceAdapter()
      : new (await import("./browser")).BrowserVoiceAdapter();
    adapterInstance = adapter;
    return adapter;
  })();
  creating = attempt;
  // Settled either way: the next call starts over. A failed load (for
  // example a chunk that did not download) must not stick until reload.
  const settle = () => {
    if (creating === attempt) creating = null;
  };
  attempt.then(settle, settle);
  return attempt;
}

/**
 * Reset the voice adapter (for testing or cleanup)
 */
export function resetVoiceAdapter(): void {
  if (adapterInstance) {
    adapterInstance.dispose();
    adapterInstance = null;
  }
}

/**
 * Get the current voice adapter instance (if exists)
 */
export function getVoiceAdapter(): VoiceAdapter | null {
  return adapterInstance;
}

// Re-export types
export * from "./types";
