/**
 * Call Ring Service
 *
 * Manages the looping ring sound for incoming calls.
 * Uses Web Audio API for precise looping and volume control.
 */

import { getSoundVolume, getSoundEnabled } from "@/stores/sound";

// ============================================================================
// Constants
// ============================================================================

// Use "bell" sound for ringing (can be replaced with a dedicated ring sound later)
const RING_SOUND_PATH = "/sounds/bell.wav";
const RING_INTERVAL_MS = 2000; // Ring every 2 seconds

// ============================================================================
// State
// ============================================================================

let ringInterval: ReturnType<typeof setInterval> | null = null;
let audioContext: AudioContext | null = null;
let ringBuffer: AudioBuffer | null = null;
let currentSource: AudioBufferSourceNode | null = null;
let gainNode: GainNode | null = null;

// ============================================================================
// Initialization
// ============================================================================

/**
 * Get or create the AudioContext for ring sounds.
 */
function getAudioContext(): AudioContext {
  if (!audioContext) {
    audioContext = new AudioContext();
  }
  return audioContext;
}

/**
 * Load the ring sound buffer.
 */
async function loadRingBuffer(): Promise<AudioBuffer | null> {
  if (ringBuffer) return ringBuffer;

  try {
    const ctx = getAudioContext();
    const response = await fetch(RING_SOUND_PATH);
    if (!response.ok) {
      console.warn(`Failed to fetch ring sound: ${RING_SOUND_PATH}`);
      return null;
    }
    const arrayBuffer = await response.arrayBuffer();
    ringBuffer = await ctx.decodeAudioData(arrayBuffer);
    return ringBuffer;
  } catch (error) {
    console.warn("Failed to load ring sound:", error);
    return null;
  }
}

// ============================================================================
// Playback
// ============================================================================

/**
 * Play the ring sound once.
 */
async function playRingOnce(): Promise<void> {
  // Check if sounds are enabled
  if (!getSoundEnabled()) return;

  try {
    const ctx = getAudioContext();

    // Resume if suspended
    if (ctx.state === "suspended") {
      await ctx.resume();
    }

    // Load buffer if needed
    const buffer = await loadRingBuffer();
    if (!buffer) return;

    // Stop any currently playing ring
    if (currentSource) {
      try {
        currentSource.stop();
      } catch {
        // Ignore errors from already stopped sources
      }
    }

    // Create nodes
    currentSource = ctx.createBufferSource();
    gainNode = ctx.createGain();

    // Configure
    currentSource.buffer = buffer;
    gainNode.gain.value = getSoundVolume() / 100;

    // Connect: source -> gain -> destination
    currentSource.connect(gainNode);
    gainNode.connect(ctx.destination);

    // Play
    currentSource.start(0);
  } catch (error) {
    console.warn("Failed to play ring sound:", error);
  }
}

/**
 * Start the ring loop for an incoming call.
 */
export function startRinging(): void {
  // Don't start if already ringing
  if (ringInterval) return;

  // Play immediately
  playRingOnce();

  // Set up interval for repeated rings
  ringInterval = setInterval(() => {
    playRingOnce();
  }, RING_INTERVAL_MS);

  console.log("[Ring] Started ringing");
}

/**
 * Stop the ring loop.
 */
export function stopRinging(): void {
  // Clear interval
  if (ringInterval) {
    clearInterval(ringInterval);
    ringInterval = null;
  }

  // Stop any currently playing sound
  if (currentSource) {
    try {
      currentSource.stop();
    } catch {
      // Ignore errors from already stopped sources
    }
    currentSource = null;
  }

  console.log("[Ring] Stopped ringing");
}

/**
 * Check if currently ringing.
 */
export function isRinging(): boolean {
  return ringInterval !== null;
}

/**
 * Preload the ring sound buffer.
 * Call this after user interaction to ensure audio is ready.
 */
export async function preloadRingSound(): Promise<void> {
  await loadRingBuffer();
}
