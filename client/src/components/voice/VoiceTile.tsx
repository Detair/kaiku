/**
 * VoiceTile — Individual tile for participant or screen share in the voice grid.
 *
 * Displays either a participant (with optional webcam video) or a screen share
 * (with video track from the screenShareViewer store).
 */

import { Component, Show, createEffect, onCleanup } from "solid-js";
import { MicOff, Monitor, ExternalLink, Undo2, MonitorOff } from "lucide-solid";
import { getTrack as getWebcamTrack } from "@/stores/webcamViewer";
import { viewerState as screenShareViewerState } from "@/stores/screenShareViewer";
import TauriVideoFrame from "./TauriVideoFrame";

const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type TileData =
  | {
      type: "participant";
      userId: string;
      displayName: string;
      username: string;
      muted: boolean;
      deafened: boolean;
      speaking: boolean;
    }
  | {
      type: "screen_share";
      streamId: string;
      userId: string;
      username: string;
    };

interface VoiceTileProps {
  tile: TileData;
  onClick: () => void;
  size?: "normal" | "small";
  poppedOut?: boolean;
  onPopOut?: () => void;
  onBringBack?: () => void;
  onStopShare?: () => void;
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

/** Participant tile — shows webcam video or avatar with speaking/mute state. */
const ParticipantTile: Component<{
  tile: Extract<TileData, { type: "participant" }>;
  size: "normal" | "small";
}> = (props) => {
  let videoRef: HTMLVideoElement | undefined;

  const track = () => getWebcamTrack(props.tile.userId);
  /** Browser mode: real MediaStreamTrack available */
  const hasBrowserVideo = () => !!track();
  /** Tauri mode: webcam entry registered (track is null, video renders via canvas frames) */
  const hasTauriVideo = () => isTauri && track() === null;
  const hasVideo = () => hasBrowserVideo() || hasTauriVideo();

  createEffect(() => {
    if (!videoRef) return;
    const t = track();
    if (t) {
      videoRef.srcObject = new MediaStream([t]);
    } else {
      videoRef.srcObject = null;
    }
  });

  onCleanup(() => {
    if (videoRef) videoRef.srcObject = null;
  });

  const initial = () =>
    (
      props.tile.displayName?.charAt(0) ||
      props.tile.username?.charAt(0) ||
      "?"
    ).toUpperCase();

  const isSmall = () => props.size === "small";

  return (
    <>
      {/* Webcam video (fills tile) */}
      <Show when={hasVideo()}>
        <Show
          when={!isTauri}
          fallback={
            <div class="absolute inset-0">
              <TauriVideoFrame userId={props.tile.userId} streamId="webcam" />
            </div>
          }
        >
          <video
            ref={videoRef}
            autoplay
            playsinline
            muted
            class="absolute inset-0 w-full h-full object-cover"
          />
        </Show>
        {/* Name overlay at bottom with gradient */}
        <div class="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/70 to-transparent px-2 py-1.5">
          <span
            class="text-text-primary font-medium truncate block"
            classList={{
              "text-xs": isSmall(),
              "text-sm": !isSmall(),
            }}
          >
            {props.tile.displayName || props.tile.username}
          </span>
        </div>
      </Show>

      {/* Avatar fallback (no webcam) */}
      <Show when={!hasVideo()}>
        <div class="flex flex-col items-center justify-center h-full gap-2">
          <div
            class="rounded-full bg-surface-highlight flex items-center justify-center font-bold text-text-primary"
            classList={{
              "w-7 h-7 text-xs": isSmall(),
              "w-16 h-16 text-2xl": !isSmall(),
            }}
          >
            {initial()}
          </div>
          <span
            class="text-text-primary font-medium truncate max-w-[90%] text-center"
            classList={{
              "text-xs": isSmall(),
              "text-sm": !isSmall(),
            }}
          >
            {props.tile.displayName || props.tile.username}
          </span>
        </div>
      </Show>

      {/* Muted badge — bottom-right */}
      <Show when={props.tile.muted}>
        <div class="absolute bottom-1.5 right-1.5 bg-accent-danger/25 text-text-primary rounded p-0.5">
          <MicOff size={isSmall() ? 10 : 14} />
        </div>
      </Show>
    </>
  );
};

/** Screen share tile — shows video track or placeholder. */
const ScreenShareTile: Component<{
  tile: Extract<TileData, { type: "screen_share" }>;
  poppedOut: boolean;
  onPopOut?: () => void;
  onBringBack?: () => void;
  onStopShare?: () => void;
}> = (props) => {
  const trackInfo = () =>
    screenShareViewerState.availableTracks.get(props.tile.streamId);
  const hasTrack = () => !!trackInfo();
  /** Browser mode: real MediaStreamTrack available */
  const hasBrowserTrack = () => !!trackInfo()?.track;

  // Attach stream when video element mounts (including after bring-back remount)
  const attachStream = (el: HTMLVideoElement) => {
    const info = trackInfo();
    if (info?.track) {
      el.srcObject = new MediaStream([info.track]);
    }
    onCleanup(() => { el.srcObject = null; });
  };

  return (
    <>
      {/* Video or placeholder */}
      <Show
        when={hasTrack()}
        fallback={
          /* Loading placeholder */
          <div class="flex flex-col items-center justify-center h-full gap-2 text-text-muted">
            <Monitor size={28} />
            <span class="text-xs">Connecting...</span>
          </div>
        }
      >
        <Show
          when={!props.poppedOut}
          fallback={
            /* Popped out placeholder */
            <div class="flex flex-col items-center justify-center h-full gap-2">
              <Monitor size={28} class="text-text-muted" />
              <span class="text-xs text-text-muted">Popped out</span>
              <Show when={props.onBringBack}>
                <button
                  class="mt-1 text-xs text-text-primary hover:text-text-secondary px-2 py-1 rounded bg-surface-highlight transition-colors"
                  onClick={(e) => {
                    e.stopPropagation();
                    props.onBringBack?.();
                  }}
                >
                  <div class="flex items-center gap-1">
                    <Undo2 size={12} />
                    Bring back
                  </div>
                </button>
              </Show>
            </div>
          }
        >
          <Show
            when={hasBrowserTrack()}
            fallback={
              /* Tauri mode: render canvas-based video frame receiver */
              <div class="absolute inset-0">
                <TauriVideoFrame userId={trackInfo()!.userId} streamId={props.tile.streamId} />
              </div>
            }
          >
            <video
              ref={attachStream}
              autoplay
              playsinline
              muted
              class="absolute inset-0 w-full h-full object-contain bg-black"
            />
          </Show>
        </Show>
      </Show>

      {/* Label at bottom */}
      <div class="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/70 to-transparent px-2 py-1.5">
        <span class="text-sm text-text-primary font-medium truncate block">
          {props.tile.username}'s Screen
        </span>
      </div>

      {/* Hover overlay buttons — top-right */}
      <div class="absolute top-1.5 right-1.5 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
        <Show when={props.onStopShare}>
          <button
            class="bg-accent-danger/80 hover:bg-accent-danger text-white rounded p-1"
            onClick={(e) => {
              e.stopPropagation();
              props.onStopShare?.();
            }}
            title="Stop sharing"
          >
            <MonitorOff size={14} />
          </button>
        </Show>
        <Show when={!props.poppedOut && hasTrack() && props.onPopOut}>
          <button
            class="bg-black/50 hover:bg-black/70 text-text-primary rounded p-1"
            onClick={(e) => {
              e.stopPropagation();
              props.onPopOut?.();
            }}
            title="Pop out"
          >
            <ExternalLink size={14} />
          </button>
        </Show>
      </div>
    </>
  );
};

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

const VoiceTile: Component<VoiceTileProps> = (props) => {
  const size = () => props.size ?? "normal";
  const isParticipant = () => props.tile.type === "participant";
  const isSpeaking = () =>
    props.tile.type === "participant" && props.tile.speaking;

  const ariaLabel = () => {
    if (props.tile.type === "participant") {
      const parts = [props.tile.displayName || props.tile.username];
      if (props.tile.muted) parts.push("muted");
      if (props.tile.deafened) parts.push("deafened");
      if (props.tile.speaking) parts.push("speaking");
      return parts.join(", ");
    }
    return `${props.tile.username}'s screen share`;
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      props.onClick();
    }
  };

  return (
    <div
      role="button"
      tabindex="0"
      aria-label={ariaLabel()}
      class="group h-full rounded-xl overflow-hidden cursor-pointer transition-all duration-200 relative bg-surface-layer2"
      classList={{
        "border-2 border-accent-success shadow-[0_0_12px_rgba(67,181,129,0.3)]":
          isSpeaking(),
        "border border-surface-highlight": !isSpeaking(),
      }}
      onClick={() => props.onClick()}
      onKeyDown={handleKeyDown}
    >
      <Show
        when={isParticipant()}
        fallback={
          <ScreenShareTile
            tile={props.tile as Extract<TileData, { type: "screen_share" }>}
            poppedOut={props.poppedOut ?? false}
            onPopOut={props.onPopOut}
            onBringBack={props.onBringBack}
            onStopShare={props.onStopShare}
          />
        }
      >
        <ParticipantTile
          tile={props.tile as Extract<TileData, { type: "participant" }>}
          size={size()}
        />
      </Show>
    </div>
  );
};

export default VoiceTile;
