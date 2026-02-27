/**
 * Browser Audio Playback
 *
 * Web Audio API wrapper with preloading and Notification API fallback.
 */

import { getSoundPath, type SoundOption } from "./types";
import { getSoundVolume } from "@/stores/sound";

// ============================================================================
// Types
// ============================================================================

interface AudioCache {
  [soundId: string]: AudioBuffer;
}

// ============================================================================
// State
// ============================================================================

let audioContext: AudioContext | null = null;
const audioCache: AudioCache = {};
let preloadPromise: Promise<void> | null = null;

// ============================================================================
// Audio Context Management
// ============================================================================

/**
 * Get or create the AudioContext.
 * Must be called after user interaction due to browser autoplay policies.
 */
function getAudioContext(): AudioContext {
  if (!audioContext) {
    audioContext = new AudioContext();
  }
  return audioContext;
}

/**
 * Resume audio context if suspended (required after user interaction).
 */
export async function resumeAudioContext(): Promise<void> {
  const ctx = getAudioContext();
  if (ctx.state === "suspended") {
    await ctx.resume();
  }
}

// ============================================================================
// Preloading
// ============================================================================

/**
 * Preload all sound files into AudioBuffers.
 * Should be called on app initialization after user login.
 */
export async function preloadSounds(): Promise<void> {
  if (preloadPromise) return preloadPromise;

  const soundIds: SoundOption[] = [
    "default",
    "subtle",
    "ping",
    "chime",
    "bell",
  ];

  preloadPromise = (async () => {
    const ctx = getAudioContext();

    await Promise.all(
      soundIds.map(async (id) => {
        try {
          const path = getSoundPath(id);
          const response = await fetch(path);
          if (!response.ok) {
            console.warn(`Failed to fetch sound: ${path}`);
            return;
          }
          const arrayBuffer = await response.arrayBuffer();
          const audioBuffer = await ctx.decodeAudioData(arrayBuffer);
          audioCache[id] = audioBuffer;
        } catch (error) {
          console.warn(`Failed to preload sound ${id}:`, error);
        }
      }),
    );
  })();

  return preloadPromise;
}

// ============================================================================
// Playback
// ============================================================================

/**
 * Play a notification sound using Web Audio API.
 */
export async function playSound(soundId: SoundOption): Promise<boolean> {
  try {
    const ctx = getAudioContext();

    // Resume if suspended
    if (ctx.state === "suspended") {
      await ctx.resume();
    }

    // Get cached buffer or load on demand
    let buffer = audioCache[soundId];
    if (!buffer) {
      // Try to load on demand
      const path = getSoundPath(soundId);
      const response = await fetch(path);
      if (!response.ok) {
        console.warn(`Sound file not found: ${path}`);
        return false;
      }
      const arrayBuffer = await response.arrayBuffer();
      buffer = await ctx.decodeAudioData(arrayBuffer);
      audioCache[soundId] = buffer;
    }

    // Create nodes
    const source = ctx.createBufferSource();
    const gainNode = ctx.createGain();

    // Configure
    source.buffer = buffer;
    gainNode.gain.value = getSoundVolume() / 100;

    // Connect: source -> gain -> destination
    source.connect(gainNode);
    gainNode.connect(ctx.destination);

    // Play
    source.start(0);

    return true;
  } catch (error) {
    console.warn("Failed to play sound via Web Audio API:", error);
    return false;
  }
}

/**
 * Fallback: Use Notification API sound.
 * Limited: no volume control, browser-dependent sound.
 */
export function playNotificationFallback(): boolean {
  try {
    if (!("Notification" in window)) {
      return false;
    }

    if (Notification.permission === "granted") {
      // Create a notification with sound (browser plays default sound)
      const notification = new Notification("", {
        silent: false,
        tag: "sound-fallback",
        // Immediately close it - we just want the sound
      });
      notification.close();
      return true;
    }

    return false;
  } catch {
    return false;
  }
}

/**
 * Check if Web Audio API is available.
 */
export function isWebAudioSupported(): boolean {
  return typeof AudioContext !== "undefined" || "webkitAudioContext" in window;
}
