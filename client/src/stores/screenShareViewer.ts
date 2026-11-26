/**
 * Screen Share Viewer Store
 *
 * Manages state for viewing screen shares - which share is being viewed,
 * view mode, volume settings, grid layout, etc.
 *
 * Tracks are keyed by streamId (not userId) to support multiple streams
 * per user.
 */

import { createSignal } from "solid-js";
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
  track: MediaStreamTrack | null; // null in Tauri mode — video renders via canvas frames
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
  track: MediaStreamTrack | null,
  userId: string,
  username: string,
  sourceLabel: string,
): void {
  if (stoppedStreams.has(streamId)) {
    console.log("[ScreenShareViewer] Late track of a stopped share ignored:", streamId);
    return;
  }
  // Auto-cleanup when track ends (only for browser MediaStreamTrack)
  if (track) {
    track.onended = () => {
      removeAvailableTrack(streamId);
    };
  }

  const newTracks = new Map(viewerState.availableTracks);
  newTracks.set(streamId, { track, userId, username, sourceLabel });
  setViewerState({ availableTracks: newTracks });

  // Don't auto-open the viewer — let VoiceChannelView handle display.
  // Users click to view from VoiceChannelView's screen share section.
}

/**
 * Remove every track of `userId` (the own self-preview when the voice
 * session ends: a stopped local track fires no `ended`).
 */
export function removeTracksOfUser(userId: string): void {
  for (const [streamId, info] of viewerState.availableTracks) {
    if (info.userId === userId) removeAvailableTrack(streamId);
  }
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

  // The watched stream is gone: the viewer closes. It does not open
  // another share (or the own preview) the person did not choose.
  if (viewerState.viewingStreamId === streamId) {
    setViewerState({
      viewingStreamId: null,
      videoTrack: null,
    });
  }
}

/** A start that waits for its track; dropped by any later start or stop. */
let pendingStart: ReturnType<typeof setTimeout> | null = null;
/** The share that start waits for. */
let pendingStreamId: string | null = null;

function dropPendingStart(): void {
  if (pendingStart !== null) clearTimeout(pendingStart);
  pendingStart = null;
  pendingStreamId = null;
}

/**
 * How long a share that stopped before its first track arrived keeps that
 * late track out (share ids are new per share, so nothing else is refused).
 */
export const STOPPED_STREAM_MEMORY_MS = 10_000;

/** Shares that stopped before their first track arrived. */
const stoppedStreams = new Map<string, ReturnType<typeof setTimeout>>();

/** The voice channel the memory belongs to (set when a channel is joined). */
let memoryChannel: string | null = null;

/**
 * A voice session in `channelId` begins: nothing remembered from an earlier
 * session refuses a track of this one, and stops of other channels are not
 * remembered.
 */
export function beginStopMemory(channelId: string): void {
  for (const timer of stoppedStreams.values()) clearTimeout(timer);
  stoppedStreams.clear();
  memoryChannel = channelId;
}

/**
 * A share stopped in `channelId`. If it is the joined channel and no track
 * of it is held yet, its first track may still be on the way: it is
 * refused, and a viewer waiting for it stops waiting.
 */
export function noteStreamStopped(streamId: string, channelId: string): void {
  if (channelId !== memoryChannel) return;
  if (pendingStreamId === streamId) dropPendingStart();
  if (viewerState.availableTracks.has(streamId)) return;
  clearTimeout(stoppedStreams.get(streamId));
  stoppedStreams.set(
    streamId,
    setTimeout(() => stoppedStreams.delete(streamId), STOPPED_STREAM_MEMORY_MS),
  );
}

/**
 * Start viewing a specific screen share stream.
 * Sets the stream as the primary view. If the stream's track has not
 * arrived yet, waits for it for a few seconds; closing the viewer or
 * viewing another share ends that wait.
 */
export function startViewing(streamId: string): void {
  dropPendingStart();
  tryStartViewing(streamId, 0);
}

function tryStartViewing(streamId: string, retries: number): void {
  pendingStart = null;
  pendingStreamId = null;
  const info = viewerState.availableTracks.get(streamId);
  if (!info) {
    // The WebSocket broadcast arrives before the WebRTC track.
    // Retry a few times to wait for addAvailableTrack to register it.
    if (retries < 10) {
      pendingStart = setTimeout(() => tryStartViewing(streamId, retries + 1), 500);
      pendingStreamId = streamId;
      return;
    }
    console.warn(
      "[ScreenShareViewer] Cannot start viewing — no track for stream after retries:",
      streamId,
    );
    return;
  }

  // Check if track is still active (only for browser MediaStreamTrack)
  if (info.track && info.track.readyState === "ended") {
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
  dropPendingStart();
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

  if (info.track && info.track.readyState === "ended") {
    console.warn(
      "[ScreenShareViewer] Track has ended for stream:",
      streamId,
    );
    removeAvailableTrack(streamId);
    return;
  }

  dropPendingStart();
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

/**
 * A request to show a share as the focused tile of the voice channel view
 * (desktop: there is no browser track for the full-screen viewer).
 */
const [tileFocusRequest, setTileFocusRequest] = createSignal<string | null>(null);
export { tileFocusRequest };

/** A focus request not taken within this time is dropped (it would surprise later). */
export const TILE_FOCUS_REQUEST_TTL_MS = 10_000;
let tileFocusTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Ask the voice channel view to focus `streamId`. The request waits until
 * the view shows that share (its tile may come a moment later) or until
 * `TILE_FOCUS_REQUEST_TTL_MS` passed.
 */
export function requestTileFocus(streamId: string): void {
  clearTimeout(tileFocusTimer);
  setTileFocusRequest(streamId);
  tileFocusTimer = setTimeout(() => setTileFocusRequest(null), TILE_FOCUS_REQUEST_TTL_MS);
}

/** The view has taken the request. */
export function clearTileFocusRequest(): void {
  clearTimeout(tileFocusTimer);
  setTileFocusRequest(null);
}
