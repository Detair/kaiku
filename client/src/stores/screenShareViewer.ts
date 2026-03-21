/**
 * Screen Share Viewer Store
 *
 * Manages state for viewing screen shares - which share is being viewed,
 * view mode, volume settings, grid layout, etc.
 *
 * Tracks are keyed by streamId (not userId) to support multiple streams
 * per user.
 */

import { createStore } from "solid-js/store";

/** View mode for the screen share viewer */
export type ViewMode = "spotlight" | "pip" | "theater";

/** Layout mode: focus on one stream, or grid of up to 4 */
export type LayoutMode = "focus" | "grid";

/** Position for PiP mode */
export interface PipPosition {
  x: number;
  y: number;
}

/** Info about an available screen share track */
export interface AvailableTrackInfo {
  track: MediaStreamTrack;
  userId: string;
  username: string;
  sourceLabel: string;
}

/** Screen share viewer state */
interface ScreenShareViewerState {
  /** Stream ID of the screen share being viewed in primary view (null if none) */
  viewingStreamId: string | null;
  /** The video track being displayed in primary view */
  videoTrack: MediaStreamTrack | null;
  /** Current view mode (spotlight/pip/theater) */
  viewMode: ViewMode;

  /** Stream IDs shown in grid view (max 4) */
  gridStreamIds: string[];
  /** Layout mode: focus on one stream or grid of up to 4 */
  layoutMode: LayoutMode;

  /** Volume for screen audio (0-100) */
  screenVolume: number;
  /** Previous volume before muting (for toggle restore) */
  previousVolume: number;
  /** PiP position (for pip mode) */
  pipPosition: PipPosition;
  /** PiP size */
  pipSize: { width: number; height: number };

  /** Available screen share tracks keyed by streamId */
  availableTracks: Map<string, AvailableTrackInfo>;
}

const MAX_GRID_STREAMS = 4;
const DEFAULT_PIP_SIZE = { width: 400, height: 225 };

const [viewerState, setViewerState] = createStore<ScreenShareViewerState>({
  viewingStreamId: null,
  videoTrack: null,
  viewMode: "spotlight",
  gridStreamIds: [],
  layoutMode: "focus",
  screenVolume: 100,
  previousVolume: 100,
  pipPosition: { x: 20, y: 20 },
  pipSize: DEFAULT_PIP_SIZE,
  availableTracks: new Map(),
});

/**
 * Register an available screen share track.
 * Called when a remote user's screen share track is received.
 */
export function addAvailableTrack(
  streamId: string,
  track: MediaStreamTrack,
  userId: string,
  username: string,
  sourceLabel: string,
): void {
  // Auto-cleanup when track ends
  track.onended = () => {
    removeAvailableTrack(streamId);
  };

  const newTracks = new Map(viewerState.availableTracks);
  newTracks.set(streamId, { track, userId, username, sourceLabel });
  setViewerState({ availableTracks: newTracks });

  // Don't auto-open the viewer — let VoiceChannelView handle display.
  // Users click to view from VoiceChannelView's screen share section.
}

/**
 * Remove a screen share track (stream stopped).
 * If it was the primary view, auto-switch to next available stream.
 */
export function removeAvailableTrack(streamId: string): void {
  const newTracks = new Map(viewerState.availableTracks);
  newTracks.delete(streamId);
  setViewerState({ availableTracks: newTracks });

  // Remove from grid if present
  if (viewerState.gridStreamIds.includes(streamId)) {
    setViewerState({
      gridStreamIds: viewerState.gridStreamIds.filter((id) => id !== streamId),
    });
  }

  // If we were viewing this stream, auto-switch to next available
  if (viewerState.viewingStreamId === streamId) {
    const remaining = Array.from(newTracks.keys());
    if (remaining.length > 0) {
      const nextStreamId = remaining[0];
      const nextInfo = newTracks.get(nextStreamId)!;
      setViewerState({
        viewingStreamId: nextStreamId,
        videoTrack: nextInfo.track,
      });
    } else {
      setViewerState({
        viewingStreamId: null,
        videoTrack: null,
      });
    }
  }
}

/**
 * Start viewing a specific screen share stream.
 * Sets the stream as the primary view. If the stream is already registered
 * as available, uses the existing track; otherwise this is a no-op
 * (use addAvailableTrack first).
 */
export function startViewing(streamId: string, retries = 0): void {
  const info = viewerState.availableTracks.get(streamId);
  if (!info) {
    // The WebSocket broadcast arrives before the WebRTC track.
    // Retry a few times to wait for addAvailableTrack to register it.
    if (retries < 10) {
      setTimeout(() => startViewing(streamId, retries + 1), 500);
      return;
    }
    console.warn(
      "[ScreenShareViewer] Cannot start viewing — no track for stream after retries:",
      streamId,
    );
    return;
  }

  // Check if track is still active
  if (info.track.readyState === "ended") {
    console.warn(
      "[ScreenShareViewer] Track has ended for stream:",
      streamId,
    );
    removeAvailableTrack(streamId);
    return;
  }

  setViewerState({
    viewingStreamId: streamId,
    videoTrack: info.track,
  });
}

/**
 * Stop viewing the current screen share (clear primary view).
 */
export function stopViewing(): void {
  setViewerState({
    viewingStreamId: null,
    videoTrack: null,
  });
}

/**
 * Get list of available screen shares with metadata.
 */
export function getAvailableSharers(): {
  streamId: string;
  userId: string;
  username: string;
  sourceLabel: string;
}[] {
  return Array.from(viewerState.availableTracks.entries()).map(
    ([streamId, info]) => ({
      streamId,
      userId: info.userId,
      username: info.username,
      sourceLabel: info.sourceLabel,
    }),
  );
}

/**
 * Set the layout mode (focus or grid).
 */
export function setLayoutMode(mode: LayoutMode): void {
  setViewerState({ layoutMode: mode });
}

/**
 * Add a stream to the grid view (max 4).
 * Returns true if added, false if grid is full or stream already in grid.
 */
export function addToGrid(streamId: string): boolean {
  if (viewerState.gridStreamIds.includes(streamId)) {
    return false;
  }
  if (viewerState.gridStreamIds.length >= MAX_GRID_STREAMS) {
    return false;
  }
  // Verify the stream exists
  if (!viewerState.availableTracks.has(streamId)) {
    return false;
  }
  setViewerState({
    gridStreamIds: [...viewerState.gridStreamIds, streamId],
  });
  return true;
}

/**
 * Remove a stream from the grid view.
 */
export function removeFromGrid(streamId: string): void {
  setViewerState({
    gridStreamIds: viewerState.gridStreamIds.filter((id) => id !== streamId),
  });
}

/**
 * Swap a stream into the primary view.
 * If there's a current primary, it stays available but is no longer the focus.
 */
export function swapPrimary(streamId: string): void {
  const info = viewerState.availableTracks.get(streamId);
  if (!info) {
    console.warn(
      "[ScreenShareViewer] Cannot swap — no track for stream:",
      streamId,
    );
    return;
  }

  if (info.track.readyState === "ended") {
    console.warn(
      "[ScreenShareViewer] Track has ended for stream:",
      streamId,
    );
    removeAvailableTrack(streamId);
    return;
  }

  setViewerState({
    viewingStreamId: streamId,
    videoTrack: info.track,
  });
}

/**
 * Set the view mode (spotlight/pip/theater).
 */
export function setViewMode(mode: ViewMode): void {
  setViewerState({ viewMode: mode });
}

/**
 * Set screen audio volume (0-100).
 */
export function setScreenVolume(volume: number): void {
  setViewerState({ screenVolume: Math.max(0, Math.min(100, volume)) });
}

/**
 * Toggle mute with volume memory.
 */
export function toggleMute(): void {
  if (viewerState.screenVolume === 0) {
    setScreenVolume(viewerState.previousVolume || 100);
  } else {
    setViewerState({ previousVolume: viewerState.screenVolume });
    setScreenVolume(0);
  }
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
 * Check if currently viewing a specific stream.
 */
export function isViewingStream(streamId: string): boolean {
  return viewerState.viewingStreamId === streamId;
}

export { viewerState };
