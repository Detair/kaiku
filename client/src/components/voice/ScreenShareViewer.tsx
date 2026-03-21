import {
  Component,
  Show,
  For,
  createEffect,
  createSignal,
  onMount,
  onCleanup,
} from "solid-js";
import { Portal } from "solid-js/web";
import {
  X,
  Minimize2,
  Maximize2,
  Volume2,
  VolumeX,
  Play,
  LayoutGrid,
  Maximize,
} from "lucide-solid";
import {
  viewerState,
  stopViewing,
  setViewMode,
  setScreenVolume,
  toggleMute,
  swapPrimary,
  setLayoutMode,
  addToGrid,
  getAvailableSharers,
  type ViewMode,
  type LayoutMode,
} from "@/stores/screenShareViewer";
import { voiceState } from "@/stores/voice";
import {
  getActiveLayer,
  getLayerPreference,
  setLayerPreference,
  type Layer,
  type LayerPreference,
} from "@/stores/simulcastLayers";
import * as tauri from "@/lib/tauri";

/**
 * Screen share viewer overlay.
 * Displays the currently viewed screen share with controls.
 */
const ScreenShareViewer: Component = () => {
  let videoRef: HTMLVideoElement | undefined;
  const [autoplayBlocked, setAutoplayBlocked] = createSignal(false);
  const [contextMenu, setContextMenu] = createSignal<{
    x: number;
    y: number;
    userId: string;
    streamId: string;
  } | null>(null);

  /** Get the userId for the currently viewed stream */
  const viewingUserId = () => {
    const streamId = viewerState.viewingStreamId;
    if (!streamId) return null;
    return viewerState.availableTracks.get(streamId)?.userId ?? null;
  };

  /** Handle right-click on a video to open the quality context menu */
  const handleVideoContextMenu = (
    e: MouseEvent,
    userId: string,
    streamId: string,
  ) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, userId, streamId });
  };

  // Attach video track to video element when it changes
  createEffect(() => {
    const track = viewerState.videoTrack;
    if (track && videoRef) {
      const stream = new MediaStream([track]);
      videoRef.srcObject = stream;
      videoRef.muted = true; // Required for autoplay policy
      setAutoplayBlocked(false);
      // Delay play to let DOM settle — prevents AbortError from element re-mount
      requestAnimationFrame(() => {
        if (videoRef && videoRef.srcObject) {
          videoRef.play().catch((err) => {
            if (err.name !== "AbortError") {
              console.warn("[ScreenShareViewer] Autoplay blocked:", err);
              setAutoplayBlocked(true);
            }
          });
        }
      });
    }
  });

  // Handle manual play when autoplay is blocked
  const handleClickToPlay = () => {
    if (videoRef) {
      videoRef
        .play()
        .then(() => {
          setAutoplayBlocked(false);
        })
        .catch(console.error);
    }
  };

  // Apply volume to video element when it changes
  createEffect(() => {
    if (videoRef) {
      videoRef.volume = viewerState.screenVolume / 100;
    }
  });

  // Cleanup on unmount
  onCleanup(() => {
    if (videoRef) {
      videoRef.srcObject = null;
    }
  });

  const sharerName = () => {
    const streamId = viewerState.viewingStreamId;
    if (!streamId) return "Unknown";
    const trackInfo = viewerState.availableTracks.get(streamId);
    if (!trackInfo) return "Unknown";
    const participant = voiceState.participants[trackInfo.userId];
    const name =
      participant?.display_name ||
      participant?.username ||
      trackInfo.username ||
      trackInfo.userId.slice(0, 8);
    // Include source label if available and not just "Screen"
    if (trackInfo.sourceLabel && trackInfo.sourceLabel !== "Screen") {
      return `${name} — ${trackInfo.sourceLabel}`;
    }
    return name;
  };

  const handleClose = () => {
    stopViewing();
  };

  const cycleViewMode = () => {
    // When in grid mode, cycling view mode switches back to focus mode first
    if (viewerState.layoutMode === "grid") {
      setLayoutMode("focus");
      return;
    }
    const modes: ViewMode[] = ["spotlight", "pip", "theater"];
    const currentIndex = modes.indexOf(viewerState.viewMode);
    const nextIndex = (currentIndex + 1) % modes.length;
    setViewMode(modes[nextIndex]);
  };

  /** Toggle between focus and grid layout modes */
  const toggleLayoutMode = () => {
    if (viewerState.layoutMode === "grid") {
      setLayoutMode("focus");
    } else {
      // Auto-populate grid with available streams if empty
      if (viewerState.gridStreamIds.length === 0) {
        const sharers = getAvailableSharers();
        for (const sharer of sharers.slice(0, 4)) {
          addToGrid(sharer.streamId);
        }
      }
      setLayoutMode("grid");
    }
  };

  // Keyboard shortcuts (only active when viewing a screen share)
  const handleKeyDown = (e: KeyboardEvent) => {
    if (!viewerState.viewingStreamId) return;

    // Ignore when typing in input elements
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        stopViewing();
        break;
      case "v":
      case "V":
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          cycleViewMode();
        }
        break;
      case "m":
      case "M":
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          toggleMute();
        }
        break;
      case "f":
      case "F":
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          setLayoutMode("focus");
          setViewMode("spotlight");
        }
        break;
      case "g":
      case "G":
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          toggleLayoutMode();
        }
        break;
    }
  };

  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener("keydown", handleKeyDown);
  });

  return (
    <Show when={viewerState.viewingStreamId && viewerState.videoTrack}>
      <Portal>
        {/* Grid layout mode */}
        <Show when={viewerState.layoutMode === "grid"}>
          <GridView
            onClose={handleClose}
            onToggleLayout={toggleLayoutMode}
            onVideoContextMenu={handleVideoContextMenu}
          />
        </Show>

        {/* Focus layout mode — spotlight/pip/theater */}
        <Show when={viewerState.layoutMode === "focus"}>
          <Show when={viewerState.viewMode === "spotlight"}>
            <SpotlightView
              videoRef={(el) => (videoRef = el)}
              sharerName={sharerName()}
              onClose={handleClose}
              onCycleMode={cycleViewMode}
              onToggleLayout={toggleLayoutMode}
              autoplayBlocked={autoplayBlocked()}
              onClickToPlay={handleClickToPlay}
              onVideoContextMenu={handleVideoContextMenu}
              userId={viewingUserId()}
              streamId={viewerState.viewingStreamId}
            />
          </Show>
          <Show when={viewerState.viewMode === "pip"}>
            <PipView
              videoRef={(el) => (videoRef = el)}
              sharerName={sharerName()}
              onClose={handleClose}
              onCycleMode={cycleViewMode}
              autoplayBlocked={autoplayBlocked()}
              onClickToPlay={handleClickToPlay}
              onVideoContextMenu={handleVideoContextMenu}
              userId={viewingUserId()}
              streamId={viewerState.viewingStreamId}
            />
          </Show>
          <Show when={viewerState.viewMode === "theater"}>
            <TheaterView
              videoRef={(el) => (videoRef = el)}
              sharerName={sharerName()}
              onClose={handleClose}
              onCycleMode={cycleViewMode}
              onToggleLayout={toggleLayoutMode}
              autoplayBlocked={autoplayBlocked()}
              onClickToPlay={handleClickToPlay}
              onVideoContextMenu={handleVideoContextMenu}
              userId={viewingUserId()}
              streamId={viewerState.viewingStreamId}
            />
          </Show>
        </Show>

        {/* Quality context menu */}
        <Show when={contextMenu()}>
          {(menu) => (
            <QualityContextMenu
              userId={menu().userId}
              streamId={menu().streamId}
              x={menu().x}
              y={menu().y}
              onClose={() => setContextMenu(null)}
            />
          )}
        </Show>
      </Portal>
    </Show>
  );
};

/** Human-readable label for a simulcast layer */
const layerLabel = (layer: Layer): string => {
  switch (layer) {
    case "high":
      return "HD";
    case "medium":
      return "SD";
    case "low":
      return "LD";
  }
};

/** Quality badge overlay showing the current simulcast layer */
const QualityBadge: Component<{
  userId: string;
  streamId: string;
}> = (props) => {
  const trackSource = () => `screen_video:${props.streamId}`;
  const layer = () => getActiveLayer(props.userId, trackSource());

  return (
    <div class="absolute bottom-2 right-2 px-1.5 py-0.5 rounded bg-black/60 text-white text-xs font-medium select-none z-10">
      {layerLabel(layer())}
    </div>
  );
};

/** Quality context menu for selecting simulcast layer preference */
const QualityContextMenu: Component<{
  userId: string;
  streamId: string;
  x: number;
  y: number;
  onClose: () => void;
}> = (props) => {
  const trackSource = () => `screen_video:${props.streamId}`;
  const currentPref = () => getLayerPreference(props.userId, trackSource());

  const options: { label: string; value: LayerPreference }[] = [
    { label: "Auto", value: "auto" },
    { label: "High (HD)", value: "high" },
    { label: "Medium (SD)", value: "medium" },
    { label: "Low (LD)", value: "low" },
  ];

  const handleSelect = (pref: LayerPreference) => {
    setLayerPreference(props.userId, trackSource(), pref);
    tauri.wsSend({
      type: "voice_set_layer_preference",
      channel_id: voiceState.channelId ?? "",
      target_user_id: props.userId,
      track_source: trackSource(),
      preferred_layer: pref,
    });
    props.onClose();
  };

  // Close menu on outside click
  const handleClickOutside = (_e: MouseEvent) => {
    props.onClose();
  };

  onMount(() => {
    // Delay to avoid the menu being immediately closed by the contextmenu event.
    // Track cleanup state to prevent listener leak if component unmounts
    // before the timeout fires.
    let cleaned = false;
    setTimeout(() => {
      if (cleaned) return;
      window.addEventListener("click", handleClickOutside);
      window.addEventListener("contextmenu", handleClickOutside);
    }, 0);

    onCleanup(() => {
      cleaned = true;
      window.removeEventListener("click", handleClickOutside);
      window.removeEventListener("contextmenu", handleClickOutside);
    });
  });

  return (
    <div
      class="fixed z-[100] bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-36"
      style={{ left: `${props.x}px`, top: `${props.y}px` }}
    >
      <div class="px-3 py-1 text-xs text-zinc-400 font-medium">Quality</div>
      <For each={options}>
        {(option) => (
          <button
            class={`w-full text-left px-3 py-1.5 text-sm hover:bg-zinc-700 transition-colors ${
              currentPref() === option.value
                ? "text-blue-400 font-medium"
                : "text-white"
            }`}
            onClick={() => handleSelect(option.value)}
          >
            {option.label}
            <Show when={currentPref() === option.value}>
              <span class="ml-2 text-xs text-blue-400">●</span>
            </Show>
          </button>
        )}
      </For>
    </div>
  );
};

/** Click-to-play overlay for autoplay blocked videos */
const ClickToPlayOverlay: Component<{
  onClickToPlay: () => void;
}> = (props) => (
  <div
    class="absolute inset-0 flex items-center justify-center bg-black/60 cursor-pointer z-10"
    onClick={props.onClickToPlay}
  >
    <div class="flex flex-col items-center gap-2 text-white">
      <div class="p-4 rounded-full bg-white/20 hover:bg-white/30 transition-colors">
        <Play class="w-12 h-12" />
      </div>
      <span class="text-sm">Click to play</span>
    </div>
  </div>
);

/** Spotlight mode - full screen overlay */
const SpotlightView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
  onToggleLayout: () => void;
  autoplayBlocked: boolean;
  onClickToPlay: () => void;
  onVideoContextMenu: (e: MouseEvent, userId: string, streamId: string) => void;
  userId: string | null;
  streamId: string | null;
}> = (props) => {
  return (
    <div class="fixed inset-0 z-50 bg-black flex flex-col">
      {/* Header bar */}
      <div class="flex items-center justify-between p-4 bg-black/50">
        <div class="flex items-center gap-2">
          <span class="text-white font-medium">
            {props.sharerName}'s Screen
          </span>
        </div>
        <div class="flex items-center gap-2">
          <VolumeControl />
          <LayoutToggleButton
            layoutMode={viewerState.layoutMode}
            onToggle={props.onToggleLayout}
          />
          <button
            onClick={props.onCycleMode}
            class="p-2 text-white/70 hover:text-white transition-colors"
            title="Change view mode"
          >
            <Minimize2 class="w-5 h-5" />
          </button>
          <button
            onClick={props.onClose}
            class="p-2 text-white/70 hover:text-white transition-colors"
            title="Close"
          >
            <X class="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* Video container */}
      <div
        class="flex-1 flex items-center justify-center p-4 relative"
        onContextMenu={(e) => {
          if (props.userId && props.streamId) {
            props.onVideoContextMenu(e, props.userId, props.streamId);
          }
        }}
      >
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
        <Show when={props.autoplayBlocked}>
          <ClickToPlayOverlay onClickToPlay={props.onClickToPlay} />
        </Show>
        <Show when={props.userId && props.streamId}>
          <QualityBadge userId={props.userId!} streamId={props.streamId!} />
        </Show>
      </div>

      {/* Thumbnail strip for other available streams */}
      <ThumbnailStrip />
    </div>
  );
};

/** PiP mode - small draggable window */
const PipView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
  autoplayBlocked: boolean;
  onClickToPlay: () => void;
  onVideoContextMenu: (e: MouseEvent, userId: string, streamId: string) => void;
  userId: string | null;
  streamId: string | null;
}> = (props) => {
  return (
    <div
      class="fixed z-50 bg-black rounded-lg shadow-2xl overflow-hidden"
      style={{
        right: `${viewerState.pipPosition.x}px`,
        bottom: `${viewerState.pipPosition.y}px`,
        width: `${viewerState.pipSize.width}px`,
        height: `${viewerState.pipSize.height}px`,
      }}
      onContextMenu={(e) => {
        if (props.userId && props.streamId) {
          props.onVideoContextMenu(e, props.userId, props.streamId);
        }
      }}
    >
      {/* Header */}
      <div class="absolute top-0 left-0 right-0 flex items-center justify-between p-2 bg-gradient-to-b from-black/80 to-transparent z-20">
        <span class="text-white text-xs truncate">{props.sharerName}</span>
        <div class="flex items-center gap-1">
          <button
            onClick={props.onCycleMode}
            class="p-1 text-white/70 hover:text-white transition-colors"
            title="Expand"
          >
            <Maximize2 class="w-4 h-4" />
          </button>
          <button
            onClick={props.onClose}
            class="p-1 text-white/70 hover:text-white transition-colors"
            title="Close"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Video */}
      <video
        ref={props.videoRef}
        autoplay
        playsinline
        class="w-full h-full object-contain"
      />
      <Show when={props.autoplayBlocked}>
        <ClickToPlayOverlay onClickToPlay={props.onClickToPlay} />
      </Show>
      <Show when={props.userId && props.streamId}>
        <QualityBadge userId={props.userId!} streamId={props.streamId!} />
      </Show>
    </div>
  );
};

/** Theater mode - wide view with sidebar space */
const TheaterView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
  onToggleLayout: () => void;
  autoplayBlocked: boolean;
  onClickToPlay: () => void;
  onVideoContextMenu: (e: MouseEvent, userId: string, streamId: string) => void;
  userId: string | null;
  streamId: string | null;
}> = (props) => {
  return (
    <div class="fixed top-0 left-[calc(72px+240px)] right-0 bottom-0 z-40 bg-black/95 flex flex-col">
      {/* Header bar */}
      <div class="flex items-center justify-between p-3 bg-black/50">
        <span class="text-white font-medium text-sm">
          {props.sharerName}'s Screen
        </span>
        <div class="flex items-center gap-2">
          <VolumeControl />
          <LayoutToggleButton
            layoutMode={viewerState.layoutMode}
            onToggle={props.onToggleLayout}
          />
          <button
            onClick={props.onCycleMode}
            class="p-1.5 text-white/70 hover:text-white transition-colors"
            title="Change view mode"
          >
            <Minimize2 class="w-4 h-4" />
          </button>
          <button
            onClick={props.onClose}
            class="p-1.5 text-white/70 hover:text-white transition-colors"
            title="Close"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Video container */}
      <div
        class="flex-1 flex items-center justify-center p-2 relative"
        onContextMenu={(e) => {
          if (props.userId && props.streamId) {
            props.onVideoContextMenu(e, props.userId, props.streamId);
          }
        }}
      >
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
        <Show when={props.autoplayBlocked}>
          <ClickToPlayOverlay onClickToPlay={props.onClickToPlay} />
        </Show>
        <Show when={props.userId && props.streamId}>
          <QualityBadge userId={props.userId!} streamId={props.streamId!} />
        </Show>
      </div>

      {/* Thumbnail strip for other available streams */}
      <ThumbnailStrip />
    </div>
  );
};

/** Thumbnail strip showing other available streams below the primary video */
const ThumbnailStrip: Component = () => {
  const sharers = () => getAvailableSharers();
  const others = () =>
    sharers().filter((s) => s.streamId !== viewerState.viewingStreamId);

  return (
    <Show when={others().length > 0}>
      <div class="flex gap-2 p-2 bg-zinc-900/90 overflow-x-auto">
        <For each={others()}>
          {(sharer) => {
            const sharerDisplayName = () => {
              const participant = voiceState.participants[sharer.userId];
              return (
                participant?.display_name ||
                participant?.username ||
                sharer.username ||
                sharer.userId.slice(0, 8)
              );
            };

            const label = () => {
              if (sharer.sourceLabel && sharer.sourceLabel !== "Screen") {
                return `${sharerDisplayName()} — ${sharer.sourceLabel}`;
              }
              return sharerDisplayName();
            };

            return (
              <button
                class="flex-shrink-0 w-40 h-24 rounded border border-zinc-700 hover:border-blue-500 relative overflow-hidden bg-black transition-colors"
                onClick={() => swapPrimary(sharer.streamId)}
                title={`Switch to ${label()}`}
              >
                <video
                  ref={(el) => {
                    const info = viewerState.availableTracks.get(
                      sharer.streamId,
                    );
                    if (info && el) {
                      const stream = new MediaStream([info.track]);
                      el.srcObject = stream;
                    }
                  }}
                  autoplay
                  muted
                  playsinline
                  class="w-full h-full object-contain"
                />
                <div class="absolute bottom-0 left-0 right-0 bg-black/70 text-xs px-1 py-0.5 truncate text-white">
                  {label()}
                </div>
              </button>
            );
          }}
        </For>
      </div>
    </Show>
  );
};

/** Layout toggle button — switches between focus and grid modes */
const LayoutToggleButton: Component<{
  layoutMode: LayoutMode;
  onToggle: () => void;
}> = (props) => {
  return (
    <button
      onClick={props.onToggle}
      class="p-2 text-white/70 hover:text-white transition-colors"
      title={
        props.layoutMode === "focus"
          ? "Switch to grid view (G)"
          : "Switch to focus view (G)"
      }
    >
      {props.layoutMode === "focus" ? (
        <LayoutGrid class="w-5 h-5" />
      ) : (
        <Maximize class="w-5 h-5" />
      )}
    </button>
  );
};

/** Grid view — show up to 4 streams in a 2x2 grid */
const GridView: Component<{
  onClose: () => void;
  onToggleLayout: () => void;
  onVideoContextMenu: (e: MouseEvent, userId: string, streamId: string) => void;
}> = (props) => {
  const streams = () =>
    viewerState.gridStreamIds
      .map((id) => {
        const info = viewerState.availableTracks.get(id);
        if (!info) return null;
        return { streamId: id, ...info };
      })
      .filter(
        (
          s,
        ): s is {
          streamId: string;
          track: MediaStreamTrack;
          userId: string;
          username: string;
          sourceLabel: string;
        } => s !== null,
      );

  const count = () => streams().length;

  /** Resolve a display name for a stream entry */
  const streamDisplayName = (entry: {
    userId: string;
    username: string;
    sourceLabel: string;
  }) => {
    const participant = voiceState.participants[entry.userId];
    const name =
      participant?.display_name ||
      participant?.username ||
      entry.username ||
      entry.userId.slice(0, 8);
    if (entry.sourceLabel && entry.sourceLabel !== "Screen") {
      return `${name} — ${entry.sourceLabel}`;
    }
    return name;
  };

  /** CSS grid classes based on stream count */
  const gridClasses = () => {
    const c = count();
    if (c <= 1) return "grid-cols-1";
    if (c === 2) return "grid-cols-2 grid-rows-1";
    // 3 or 4 streams: 2x2 grid
    return "grid-cols-2 grid-rows-2";
  };

  return (
    <div class="fixed inset-0 z-50 bg-black flex flex-col">
      {/* Grid container */}
      <div class={`flex-1 grid gap-1 p-1 ${gridClasses()}`}>
        <For each={streams()}>
          {(stream, index) => {
            // For 3 streams, center the last item by spanning 2 columns
            const shouldSpan = () => count() === 3 && index() === 2;

            return (
              <div
                class={`relative bg-black flex items-center justify-center overflow-hidden ${
                  shouldSpan() ? "col-span-2 max-w-[50%] mx-auto w-full" : ""
                }`}
                onContextMenu={(e) =>
                  props.onVideoContextMenu(e, stream.userId, stream.streamId)
                }
              >
                <video
                  ref={(el) => {
                    if (stream && el) {
                      const mediaStream = new MediaStream([stream.track]);
                      el.srcObject = mediaStream;
                    }
                  }}
                  autoplay
                  muted
                  playsinline
                  class="max-w-full max-h-full object-contain"
                />
                <div class="absolute bottom-2 left-2 bg-black/70 text-sm px-2 py-1 rounded text-white">
                  {streamDisplayName(stream)}
                </div>
                <QualityBadge userId={stream.userId} streamId={stream.streamId} />
              </div>
            );
          }}
        </For>
      </div>

      {/* Footer bar with controls */}
      <div class="p-2 bg-zinc-900/90 flex items-center justify-between">
        <VolumeControl />
        <div class="flex items-center gap-2">
          <LayoutToggleButton
            layoutMode={viewerState.layoutMode}
            onToggle={props.onToggleLayout}
          />
          <button
            onClick={props.onClose}
            class="p-2 text-white/70 hover:text-white transition-colors"
            title="Close"
          >
            <X class="w-5 h-5" />
          </button>
        </div>
      </div>
    </div>
  );
};

/** Volume control component */
const VolumeControl: Component = () => {
  const isMuted = () => viewerState.screenVolume === 0;

  return (
    <div class="flex items-center gap-2">
      <button
        onClick={toggleMute}
        class="p-1.5 text-white/70 hover:text-white transition-colors"
        title={isMuted() ? "Unmute" : "Mute"}
      >
        {isMuted() ? <VolumeX class="w-4 h-4" /> : <Volume2 class="w-4 h-4" />}
      </button>
      <input
        type="range"
        min="0"
        max="100"
        value={viewerState.screenVolume}
        onInput={(e) => setScreenVolume(parseInt(e.currentTarget.value))}
        class="w-20 h-1 bg-white/30 rounded-full appearance-none cursor-pointer
               [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:h-3
               [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white
               [&::-webkit-slider-thumb]:cursor-pointer [&::-webkit-slider-thumb]:transition-transform
               [&::-webkit-slider-thumb]:hover:scale-125
               [&::-moz-range-thumb]:w-3 [&::-moz-range-thumb]:h-3 [&::-moz-range-thumb]:rounded-full
               [&::-moz-range-thumb]:bg-white [&::-moz-range-thumb]:cursor-pointer [&::-moz-range-thumb]:border-0"
      />
    </div>
  );
};

export default ScreenShareViewer;
