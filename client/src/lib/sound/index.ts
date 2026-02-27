/**
 * Sound Service
 *
 * Central service for notification sounds with platform detection,
 * cooldown throttling, focus mode policy evaluation, and eligibility checking.
 * Focus policy (DND, focus mode suppression, VIP/emergency overrides) is the
 * first gate in the notification pipeline — see evaluateFocusPolicy in stores/focus.
 */

import type { SoundEvent, SoundOption } from "./types";
import {
  playSound,
  preloadSounds,
  isWebAudioSupported,
  playNotificationFallback,
} from "./browser";
import {
  initTabLeader,
  isTabLeader,
  cleanup as cleanupTabLeader,
} from "./tab-leader";
import { preloadRingSound, stopRinging } from "./ring";
import {
  getSoundEnabled,
  getSoundVolume,
  getSelectedSound,
  getChannelNotificationLevel,
  isChannelMuted,
} from "@/stores/sound";
import { evaluateFocusPolicy } from "@/stores/focus";
import { currentUser } from "@/stores/auth";

// ============================================================================
// Constants
// ============================================================================

const COOLDOWN_MS = 2000; // 2 seconds between notifications

// ============================================================================
// Platform Detection
// ============================================================================

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

// ============================================================================
// State
// ============================================================================

let initialized = false;
let lastSoundTime = 0;
let pendingSoundEvent: SoundEvent | null = null;

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initialize the sound service.
 * Should be called after user login.
 */
export async function initSoundService(): Promise<void> {
  if (initialized) return;
  initialized = true;

  // Initialize tab leadership for web
  if (!isTauri()) {
    initTabLeader();
  }

  // Preload sounds
  if (isWebAudioSupported()) {
    await preloadSounds();
    await preloadRingSound();
  }

  // Handle pending sound if AudioContext was suspended
  if (typeof document !== "undefined") {
    const playPending = async () => {
      if (pendingSoundEvent) {
        const event = pendingSoundEvent;
        pendingSoundEvent = null;
        // Only play if within 5 seconds of original event
        if (Date.now() - lastSoundTime < 5000) {
          await playSoundInternal(event);
        }
      }
    };

    document.addEventListener("click", playPending, { once: true });
    document.addEventListener("keydown", playPending, { once: true });
  }
}

/**
 * Cleanup sound service resources.
 */
export function cleanupSoundService(): void {
  // Stop any active ring
  stopRinging();

  if (!isTauri()) {
    cleanupTabLeader();
  }
  initialized = false;
}

// ============================================================================
// Playback
// ============================================================================

/**
 * Play a notification sound for the given event.
 * Handles eligibility checking, cooldown, and platform routing.
 */
export async function playNotification(event: SoundEvent): Promise<void> {
  // Quick exit: focus policy (handles DND/quiet hours, focus mode, VIP overrides)
  if (evaluateFocusPolicy(event) === "suppress") {
    console.debug("[Sound] Suppressed by focus policy");
    return;
  }

  // Quick exit: sounds disabled globally
  if (!getSoundEnabled()) {
    return;
  }

  // Quick exit: skip own messages
  const user = currentUser();
  if (event.authorId && user && event.authorId === user.id) {
    return;
  }

  // Quick exit: channel/DM muted
  if (isChannelMuted(event.channelId)) {
    return;
  }

  // Check eligibility based on channel settings and event type
  if (!isEligible(event)) {
    return;
  }

  // Check cooldown
  const now = Date.now();
  if (now - lastSoundTime < COOLDOWN_MS) {
    return;
  }

  // For web: only leader tab plays sounds
  if (!isTauri() && !isTabLeader()) {
    return;
  }

  // Play the sound
  lastSoundTime = now;
  await playSoundInternal(event);
}

/**
 * Check if an event is eligible for sound notification.
 */
function isEligible(event: SoundEvent): boolean {
  const level = getChannelNotificationLevel(event.channelId, event.isDm);

  switch (level) {
    case "all":
      // All messages trigger sound
      return true;

    case "mentions":
      // Only DMs and mentions trigger sound
      return event.isDm || event.mentionType !== undefined;

    case "none":
      // Muted
      return false;

    default:
      return false;
  }
}

/**
 * Internal playback implementation.
 */
async function playSoundInternal(_event: SoundEvent): Promise<void> {
  const soundId = getSelectedSound();

  if (isTauri()) {
    // Use Tauri native audio
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("play_sound", {
        soundId,
        volume: Math.round(getSoundVolume()),
      });
    } catch (error) {
      console.warn("Failed to play sound via Tauri:", error);
    }
  } else {
    // Use Web Audio API
    const played = await playSound(soundId);
    if (!played) {
      // Fallback to Notification API
      playNotificationFallback();
    }
  }
}

/**
 * Test sound playback (for settings UI).
 */
export async function testSound(soundId?: SoundOption): Promise<void> {
  const id = soundId ?? getSelectedSound();

  if (isTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("play_sound", {
        soundId: id,
        volume: Math.round(getSoundVolume()),
      });
    } catch (error) {
      console.warn("Failed to test sound via Tauri:", error);
    }
  } else {
    await playSound(id);
  }
}

// ============================================================================
// Re-exports
// ============================================================================

export * from "./types";
export { isTabLeader };
export { startRinging, stopRinging, isRinging } from "./ring";
