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
}

const DEFAULT_PIP_SIZE = { width: 400, height: 225 };

const [viewerState, setViewerState] = createStore<ScreenShareViewerState>({
  viewingUserId: null,
  videoTrack: null,
  viewMode: "spotlight",
  screenVolume: 100,
  pipPosition: { x: 20, y: 20 },
  pipSize: DEFAULT_PIP_SIZE,
});

/**
 * Start viewing a screen share.
 */
export function startViewing(userId: string, track: MediaStreamTrack): void {
  console.log("[ScreenShareViewer] Start viewing:", userId);
  setViewerState({
    viewingUserId: userId,
    videoTrack: track,
  });
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
