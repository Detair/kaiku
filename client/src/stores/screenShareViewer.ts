/**
 * Screen Share Viewer Store
 *
 * Manages state for viewing screen shares - which share is being viewed,
 * view mode, volume settings, etc.
 */

import { createStore } from "solid-js/store";

/** View mode for the screen share viewer */
export type ViewMode = "spotlight" | "pip" | "theater";

/** Position for PiP mode */
export interface PipPosition {
  x: number;
  y: number;
}

/** Screen share viewer state */
interface ScreenShareViewerState {
  /** User ID of the screen share being viewed (null if none) */
  viewingUserId: string | null;
  /** The video track being displayed */
  videoTrack: MediaStreamTrack | null;
  /** Current view mode */
  viewMode: ViewMode;
  /** Volume for screen audio (0-100) */
  screenVolume: number;
  /** PiP position (for pip mode) */
  pipPosition: PipPosition;
  /** PiP size */
  pipSize: { width: number; height: number };
  /** Available screen share tracks by user ID */
  availableTracks: Map<string, MediaStreamTrack>;
}

const DEFAULT_PIP_SIZE = { width: 400, height: 225 };

const [viewerState, setViewerState] = createStore<ScreenShareViewerState>({
  viewingUserId: null,
  videoTrack: null,
  viewMode: "spotlight",
  screenVolume: 100,
  pipPosition: { x: 20, y: 20 },
  pipSize: DEFAULT_PIP_SIZE,
  availableTracks: new Map(),
});

/**
 * Register an available screen share track.
 * Called when a remote user's screen share track is received.
 */
export function addAvailableTrack(
  userId: string,
  track: MediaStreamTrack,
): void {
  console.log("[ScreenShareViewer] Track available:", userId);

  // Auto-cleanup when track ends
  track.onended = () => {
    console.log("[ScreenShareViewer] Track ended for user:", userId);
    removeAvailableTrack(userId);
  };

  const newTracks = new Map(viewerState.availableTracks);
  newTracks.set(userId, track);
  setViewerState({ availableTracks: newTracks });
}

/**
 * Remove a screen share track (user stopped sharing).
 */
export function removeAvailableTrack(userId: string): void {
  console.log("[ScreenShareViewer] Track removed:", userId);
  const newTracks = new Map(viewerState.availableTracks);
  newTracks.delete(userId);
  setViewerState({ availableTracks: newTracks });

  // If we were viewing this user, stop viewing
  if (viewerState.viewingUserId === userId) {
    stopViewing();
  }
}

/**
 * Start viewing a screen share.
 * Also registers the track as available.
 */
export function startViewing(userId: string, track: MediaStreamTrack): void {
  console.log("[ScreenShareViewer] Start viewing:", userId);

  // Auto-cleanup when track ends
  track.onended = () => {
    console.log("[ScreenShareViewer] Track ended for user:", userId);
    removeAvailableTrack(userId);
  };

  // Register track as available
  const newTracks = new Map(viewerState.availableTracks);
  newTracks.set(userId, track);

  setViewerState({
    viewingUserId: userId,
    videoTrack: track,
    availableTracks: newTracks,
  });
}

/**
 * View a specific user's screen share by looking up their track.
 * Returns true if successful, false if no active track available.
 */
export function viewUserShare(userId: string): boolean {
  const track = viewerState.availableTracks.get(userId);
  if (!track) {
    console.warn("[ScreenShareViewer] No track available for user:", userId);
    return false;
  }

  // Check if track is still active
  if (track.readyState === "ended") {
    console.warn("[ScreenShareViewer] Track has ended for user:", userId);
    removeAvailableTrack(userId);
    return false;
  }

  console.log("[ScreenShareViewer] Switching to view:", userId);
  setViewerState({
    viewingUserId: userId,
    videoTrack: track,
  });
  return true;
}

/**
 * Stop viewing the current screen share.
 */
export function stopViewing(): void {
  console.log("[ScreenShareViewer] Stop viewing");
  setViewerState({
    viewingUserId: null,
    videoTrack: null,
  });
}

/**
 * Get list of users with available screen shares.
 */
export function getAvailableSharers(): string[] {
  return Array.from(viewerState.availableTracks.keys());
}

/**
 * Set the view mode.
 */
export function setViewMode(mode: ViewMode): void {
  console.log("[ScreenShareViewer] Set view mode:", mode);
  setViewerState({ viewMode: mode });
}

/**
 * Set screen audio volume (0-100).
 */
export function setScreenVolume(volume: number): void {
  setViewerState({ screenVolume: Math.max(0, Math.min(100, volume)) });
}

/**
 * Update PiP position.
 */
export function setPipPosition(position: PipPosition): void {
  setViewerState({ pipPosition: position });
}

/**
 * Update PiP size.
 */
export function setPipSize(size: { width: number; height: number }): void {
  setViewerState({ pipSize: size });
}

/**
 * Check if currently viewing a specific user's screen share.
 */
export function isViewing(userId: string): boolean {
  return viewerState.viewingUserId === userId;
}

export { viewerState };
