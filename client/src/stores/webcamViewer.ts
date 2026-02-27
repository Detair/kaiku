/**
 * Webcam Viewer Store
 *
 * Manages state for received webcam video tracks from remote participants.
 */

import { createStore } from "solid-js/store";

/** Webcam viewer state */
interface WebcamViewerState {
  /** Available webcam tracks by user ID */
  availableTracks: Map<string, MediaStreamTrack>;
}

const [viewerState, setViewerState] = createStore<WebcamViewerState>({
  availableTracks: new Map(),
});

/**
 * Register an available webcam track.
 * Called when a remote user's webcam track is received.
 */
export function addAvailableTrack(
  userId: string,
  track: MediaStreamTrack,
): void {
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
export function getTrack(userId: string): MediaStreamTrack | undefined {
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
  setViewerState({ availableTracks: new Map() });
}

export { viewerState };
