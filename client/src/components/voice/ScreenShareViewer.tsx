import { Component, Show, createEffect, createSignal, onMount, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Minimize2, Maximize2, Volume2, VolumeX, Play } from "lucide-solid";
import {
  viewerState,
  stopViewing,
  setViewMode,
  setScreenVolume,
  type ViewMode,
} from "@/stores/screenShareViewer";
import { voiceState } from "@/stores/voice";

/**
 * Screen share viewer overlay.
 * Displays the currently viewed screen share with controls.
 */
const ScreenShareViewer: Component = () => {
  let videoRef: HTMLVideoElement | undefined;
  const [autoplayBlocked, setAutoplayBlocked] = createSignal(false);

  // Attach video track to video element when it changes
  createEffect(() => {
    const track = viewerState.videoTrack;
    if (track && videoRef) {
      const stream = new MediaStream([track]);
      videoRef.srcObject = stream;
      setAutoplayBlocked(false);
      videoRef.play().catch((err) => {
        console.warn("[ScreenShareViewer] Autoplay blocked:", err);
        setAutoplayBlocked(true);
      });
    }
  });

  // Handle manual play when autoplay is blocked
  const handleClickToPlay = () => {
    if (videoRef) {
      videoRef.play().then(() => {
        setAutoplayBlocked(false);
      }).catch(console.error);
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
    const userId = viewerState.viewingUserId;
    if (!userId) return "Unknown";
    const participant = voiceState.participants[userId];
    return participant?.display_name || participant?.username || userId.slice(0, 8);
  };

  const handleClose = () => {
    stopViewing();
  };

  const cycleViewMode = () => {
    const modes: ViewMode[] = ["spotlight", "pip", "theater"];
    const currentIndex = modes.indexOf(viewerState.viewMode);
    const nextIndex = (currentIndex + 1) % modes.length;
    setViewMode(modes[nextIndex]);
  };

  // Keyboard shortcuts (only active when viewing a screen share)
  const handleKeyDown = (e: KeyboardEvent) => {
    if (!viewerState.viewingUserId) return;

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
          setScreenVolume(viewerState.screenVolume === 0 ? 100 : 0);
        }
        break;
      case "f":
      case "F":
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          setViewMode("spotlight");
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
    <Show when={viewerState.viewingUserId && viewerState.videoTrack}>
      <Portal>
        <Show when={viewerState.viewMode === "spotlight"}>
          <SpotlightView
            videoRef={(el) => (videoRef = el)}
            sharerName={sharerName()}
            onClose={handleClose}
            onCycleMode={cycleViewMode}
            autoplayBlocked={autoplayBlocked()}
            onClickToPlay={handleClickToPlay}
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
          />
        </Show>
        <Show when={viewerState.viewMode === "theater"}>
          <TheaterView
            videoRef={(el) => (videoRef = el)}
            sharerName={sharerName()}
            onClose={handleClose}
            onCycleMode={cycleViewMode}
            autoplayBlocked={autoplayBlocked()}
            onClickToPlay={handleClickToPlay}
          />
        </Show>
      </Portal>
    </Show>
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
  autoplayBlocked: boolean;
  onClickToPlay: () => void;
}> = (props) => {
  return (
    <div class="fixed inset-0 z-50 bg-black flex flex-col">
      {/* Header bar */}
      <div class="flex items-center justify-between p-4 bg-black/50">
        <div class="flex items-center gap-2">
          <span class="text-white font-medium">{props.sharerName}'s Screen</span>
        </div>
        <div class="flex items-center gap-2">
          <VolumeControl />
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
      <div class="flex-1 flex items-center justify-center p-4 relative">
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
        <Show when={props.autoplayBlocked}>
          <ClickToPlayOverlay onClickToPlay={props.onClickToPlay} />
        </Show>
      </div>
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
    </div>
  );
};

/** Theater mode - wide view with sidebar space */
const TheaterView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
  autoplayBlocked: boolean;
  onClickToPlay: () => void;
}> = (props) => {
  return (
    <div class="fixed top-0 left-[312px] right-0 bottom-0 z-40 bg-black/95 flex flex-col">
      {/* Header bar */}
      <div class="flex items-center justify-between p-3 bg-black/50">
        <span class="text-white font-medium text-sm">{props.sharerName}'s Screen</span>
        <div class="flex items-center gap-2">
          <VolumeControl />
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
      <div class="flex-1 flex items-center justify-center p-2 relative">
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
        <Show when={props.autoplayBlocked}>
          <ClickToPlayOverlay onClickToPlay={props.onClickToPlay} />
        </Show>
      </div>
    </div>
  );
};

/** Volume control component */
const VolumeControl: Component = () => {
  const isMuted = () => viewerState.screenVolume === 0;

  const toggleMute = () => {
    setScreenVolume(isMuted() ? 100 : 0);
  };

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
