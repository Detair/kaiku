import { Component, Show, createEffect, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Minimize2, Maximize2, Volume2, VolumeX } from "lucide-solid";
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

  // Attach video track to video element when it changes
  createEffect(() => {
    const track = viewerState.videoTrack;
    if (track && videoRef) {
      const stream = new MediaStream([track]);
      videoRef.srcObject = stream;
      videoRef.play().catch(console.error);
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

  return (
    <Show when={viewerState.viewingUserId && viewerState.videoTrack}>
      <Portal>
        <Show when={viewerState.viewMode === "spotlight"}>
          <SpotlightView
            videoRef={(el) => (videoRef = el)}
            sharerName={sharerName()}
            onClose={handleClose}
            onCycleMode={cycleViewMode}
          />
        </Show>
        <Show when={viewerState.viewMode === "pip"}>
          <PipView
            videoRef={(el) => (videoRef = el)}
            sharerName={sharerName()}
            onClose={handleClose}
            onCycleMode={cycleViewMode}
          />
        </Show>
        <Show when={viewerState.viewMode === "theater"}>
          <TheaterView
            videoRef={(el) => (videoRef = el)}
            sharerName={sharerName()}
            onClose={handleClose}
            onCycleMode={cycleViewMode}
          />
        </Show>
      </Portal>
    </Show>
  );
};

/** Spotlight mode - full screen overlay */
const SpotlightView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
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
      <div class="flex-1 flex items-center justify-center p-4">
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
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
      <div class="absolute top-0 left-0 right-0 flex items-center justify-between p-2 bg-gradient-to-b from-black/80 to-transparent z-10">
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
    </div>
  );
};

/** Theater mode - wide view with sidebar space */
const TheaterView: Component<{
  videoRef: (el: HTMLVideoElement) => void;
  sharerName: string;
  onClose: () => void;
  onCycleMode: () => void;
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
      <div class="flex-1 flex items-center justify-center p-2">
        <video
          ref={props.videoRef}
          autoplay
          playsinline
          class="max-w-full max-h-full object-contain"
        />
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
        class="w-20 h-1 bg-white/30 rounded-full appearance-none cursor-pointer"
      />
    </div>
  );
};

export default ScreenShareViewer;
