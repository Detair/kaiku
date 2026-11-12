/**
 * Call Ring Service
 *
 * Manages the looping ring sound for incoming calls.
 * Uses Web Audio API for precise looping and volume control.
 */

import { getSoundVolume, getSoundEnabled, isDndActive } from "@/stores/sound";

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
// Autoplay policy
// ============================================================================

const UNLOCK_EVENTS = ["pointerdown", "keydown"] as const;

/**
 * Create (or resume) the ring's AudioContext and load the ring sound. Runs on
 * the first click or key press after load: browsers only start audio after a
 * user gesture, and a context created without one stays suspended, so a call
 * arriving later would ring silently.
 */
export async function primeRingAudio(): Promise<void> {
  try {
    const ctx = getAudioContext();
    if (ctx.state === "suspended") await ctx.resume();
    await loadRingBuffer();
    // Unlocked by the gesture; it may be resumed again later without one.
    suspendWhenIdle();
  } catch (error) {
    console.warn("[Ring] Could not prepare ring audio:", error);
  }
}

/**
 * Let the audio context rest while nothing rings: a running context keeps
 * an audio device open and costs idle CPU. A ring resumes it.
 */
function suspendWhenIdle(): void {
  const ctx = audioContext;
  if (!ctx || ringInterval || ctx.state !== "running" || typeof ctx.suspend !== "function") {
    return;
  }
  ctx
    .suspend()
    .then(() => {
      // A ring that started while the context was going to sleep.
      if (ringInterval) return ctx.resume();
    })
    .catch((error: unknown) => console.warn("[Ring] Could not suspend ring audio:", error));
}

function onFirstGesture(): void {
  for (const type of UNLOCK_EVENTS) {
    document.removeEventListener(type, onFirstGesture, true);
  }
  void primeRingAudio();
}

/** Prepare the ring audio on the first user gesture (idempotent). */
export function installRingAudioUnlock(): void {
  if (typeof document === "undefined") return;
  for (const type of UNLOCK_EVENTS) {
    document.addEventListener(type, onFirstGesture, true);
  }
}

installRingAudioUnlock();

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
  // Check if DND is active - suppress ring sound but allow call UI
  if (isDndActive()) {
    console.log("[Ring] Suppressed by DND");
    return;
  }

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
  suspendWhenIdle();

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
