/**
 * Voice Adapter Factory
 *
 * Creates the appropriate voice adapter based on runtime environment
 */

import type { VoiceAdapter } from "./types";
import { BrowserVoiceAdapter } from "./browser";
import { TauriVoiceAdapter } from "./tauri";

// Singleton instance
let adapterInstance: VoiceAdapter | null = null;

/**
 * Create or return the existing voice adapter
 */
export async function createVoiceAdapter(): Promise<VoiceAdapter> {
  // Return existing instance if available
  if (adapterInstance) {
    return adapterInstance;
  }

  const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

  console.log(
    `[VoiceAdapter] Creating ${isTauri ? "Tauri" : "Browser"} adapter`,
  );

  if (isTauri) {
    adapterInstance = new TauriVoiceAdapter();
  } else {
    adapterInstance = new BrowserVoiceAdapter();
  }

  return adapterInstance;
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
