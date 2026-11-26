/**
 * Webcam Viewer Store
 *
 * Manages state for received webcam video tracks from remote participants.
 */

import { createStore } from "solid-js/store";

/** Webcam viewer state */
interface WebcamViewerState {
  /** Available webcam tracks by user ID (null track in Tauri mode — video renders via canvas frames) */
  availableTracks: Map<string, MediaStreamTrack | null>;
}

const [viewerState, setViewerState] = createStore<WebcamViewerState>({
  availableTracks: new Map(),
});

/**
 * How long a camera stop that came before the camera's first track keeps
 * that late track out (the subscriber's renegotiation may deliver it after
 * the stop).
 */
export const STOPPED_CAMERA_MEMORY_MS = 10_000;

/** People whose camera stopped before its first track arrived. */
const stoppedCameras = new Map<string, ReturnType<typeof setTimeout>>();

/** The voice channel the memory belongs to (set when a channel is joined). */
let memoryChannel: string | null = null;

function forgetStoppedCameras(): void {
  for (const timer of stoppedCameras.values()) clearTimeout(timer);
  stoppedCameras.clear();
}

/**
 * A voice session in `channelId` begins: nothing remembered from an earlier
 * session refuses a track of this one, and stops of other channels are not
 * remembered.
 */
export function beginStopMemory(channelId: string): void {
  forgetStoppedCameras();
  memoryChannel = channelId;
}

/**
 * A camera stopped in `channelId`. If it is the joined channel and no
 * picture of it is held yet, its first track may still be on the way: it
 * is refused until the person starts the camera again or the memory runs
 * out.
 */
export function noteCameraStopped(userId: string, channelId: string): void {
  if (channelId !== memoryChannel) return;
  if (viewerState.availableTracks.has(userId)) return;
  clearTimeout(stoppedCameras.get(userId));
  stoppedCameras.set(
    userId,
    setTimeout(() => stoppedCameras.delete(userId), STOPPED_CAMERA_MEMORY_MS),
  );
}

/** A new camera start of `userId`: its tracks are shown again. */
export function noteCameraStarted(userId: string): void {
  clearTimeout(stoppedCameras.get(userId));
  stoppedCameras.delete(userId);
}

/**
 * Register an available webcam track.
 * Called when a remote user's webcam track is received.
 */
export function addAvailableTrack(
  userId: string,
  track: MediaStreamTrack | null,
): void {
  if (stoppedCameras.has(userId)) {
    console.log("[WebcamViewer] Late track of a stopped camera ignored:", userId);
    return;
  }
  console.log("[WebcamViewer] Track available:", userId);

  // Note: track.onended is already set in browser.ts to call onWebcamTrackRemoved,
  // which triggers removeAvailableTrack via the voice store. Don't overwrite it here.

  const newTracks = new Map(viewerState.availableTracks);
  newTracks.set(userId, track);
  setViewerState({ availableTracks: newTracks });
}

/**
 * Remove a webcam track (user stopped webcam).
 */
export function removeAvailableTrack(userId: string): void {
  console.log("[WebcamViewer] Track removed:", userId);
  const newTracks = new Map(viewerState.availableTracks);
  newTracks.delete(userId);
  setViewerState({ availableTracks: newTracks });
}

/**
 * Get the webcam track for a specific user.
 */
export function getTrack(userId: string): MediaStreamTrack | null | undefined {
  return viewerState.availableTracks.get(userId);
}

/**
 * Get list of users with available webcam tracks.
 */
export function getWebcamUsers(): string[] {
  return Array.from(viewerState.availableTracks.keys());
}

/**
 * Clear all webcam tracks (e.g., on disconnect).
 */
export function clearAll(): void {
  forgetStoppedCameras();
  setViewerState({ availableTracks: new Map() });
}

export { viewerState };
