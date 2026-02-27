/**
 * VoiceIsland - Dynamic Voice Controls
 *
 * Floating controls inspired by Apple's Dynamic Island.
 * Appears at bottom center when connected to voice.
 *
 * Visual Indicators:
 * - Connection: Pulsing green dot + channel name + elapsed timer
 * - Mic Active: Small green activity dot (visible when unmuted)
 * - Speaking: Border glow + shadow effect + ring around mute button
 * - Screen Sharing: Highlighted button when active
 *
 * Controls:
 * - Mute/Unmute (Ctrl+Shift+M)
 * - Deafen/Undeafen (Ctrl+Shift+D)
 * - Audio Settings (opens AudioDeviceSettings modal)
 * - Screen Share (opens quality picker, toggles when active)
 * - Disconnect (red button)
 *
 * Features:
 * - Global keyboard shortcuts for mute/deafen
 * - Real-time elapsed time with formatElapsedTime utility
 * - Decoupled channel name resolution via getVoiceChannelInfo()
 */

import {
  Component,
  createSignal,
  createEffect,
  onMount,
  onCleanup,
  Show,
  lazy,
  Suspense,
} from "solid-js";
import {
  Mic,
  MicOff,
  Headphones,
  Monitor,
  PhoneOff,
  Settings,
  GripVertical,
} from "lucide-solid";
import {
  voiceState,
  setMute,
  setDeafen,
  leaveVoice,
  getVoiceChannelInfo,
  getLocalMetrics,
  stopScreenShare,
} from "@/stores/voice";
import { formatElapsedTime } from "@/lib/utils";
import { QualityIndicator } from "@/components/voice/QualityIndicator";
import { QualityTooltip } from "@/components/voice/QualityTooltip";
import { ModalFallback, LazyErrorBoundary } from "@/components/ui/LazyFallback";
import type { ConnectionMetrics } from "@/lib/webrtc/types";

const AudioDeviceSettings = lazy(
  () => import("@/components/voice/AudioDeviceSettings"),
);
const ScreenShareQualityPicker = lazy(
  () => import("@/components/voice/ScreenShareQualityPicker"),
);
const ScreenShareSourcePicker = lazy(
  () => import("@/components/voice/ScreenShareSourcePicker"),
);

const VoiceIsland: Component = () => {
  const [elapsedTime, setElapsedTime] = createSignal<string>("00:00");
  const [showSettings, setShowSettings] = createSignal(false);
  const [showQualityTooltip, setShowQualityTooltip] = createSignal(false);
  const [showScreenSharePicker, setShowScreenSharePicker] = createSignal(false);
  const [showSourcePicker, setShowSourcePicker] = createSignal(false);
  const [selectedSourceId, setSelectedSourceId] = createSignal<
    string | undefined
  >(undefined);

  // Detect if running in Tauri (native source picker available)
  const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

  // Draggable state - default to top-center
  const getInitialPosition = () => {
    // Estimate VoiceIsland width (~450px with all buttons)
    const estimatedWidth = 450;
    const centerX = (window.innerWidth - estimatedWidth) / 2;
    return {
      x: Math.max(0, centerX), // Ensure not negative
      y: 20, // 20px from top
    };
  };

  const [position, setPosition] = createSignal(getInitialPosition());
  const [isDragging, setIsDragging] = createSignal(false);
  const [dragOffset, setDragOffset] = createSignal({ x: 0, y: 0 });
  let containerRef: HTMLDivElement | undefined;

  // Timer effect - reset start time when connecting
  let startTime = Date.now();
  let timerInterval: number;

  onMount(() => {
    // Reset timer when connecting to voice
    startTime = Date.now();

    // Update timer every second using utility function
    timerInterval = window.setInterval(() => {
      if (voiceState.channelId) {
        setElapsedTime(formatElapsedTime(startTime));
      }
    }, 1000);

    // Ensure position is within bounds after mount (when actual size is known)
    if (containerRef) {
      const rect = containerRef.getBoundingClientRect();
      const currentPos = position();
      const maxX = window.innerWidth - rect.width;
      const maxY = window.innerHeight - rect.height;

      setPosition({
        x: Math.max(0, Math.min(currentPos.x, maxX)),
        y: Math.max(0, Math.min(currentPos.y, maxY)),
      });
    }
  });

  // Reset timer when connecting to a new channel
  createEffect(() => {
    if (voiceState.channelId) {
      startTime = Date.now();
      setElapsedTime("00:00");
    }
  });

  // Cleanup on unmount
  onCleanup(() => {
    if (timerInterval) {
      clearInterval(timerInterval);
    }
  });

  // Get current channel name (decoupled from channelsState)
  const channelName = () => {
    const channelInfo = getVoiceChannelInfo();
    return channelInfo?.name ?? "Voice Channel";
  };

  // Toggle mute
  const toggleMute = () => {
    setMute(!voiceState.muted);
  };

  // Toggle deafen
  const toggleDeafen = () => {
    setDeafen(!voiceState.deafened);
  };

  // Disconnect from voice
  const disconnect = async () => {
    await leaveVoice();
  };

  // Toggle screen share
  const toggleScreenShare = async () => {
    if (voiceState.screenSharing) {
      await stopScreenShare();
    } else if (isTauri) {
      // Native: show source picker first, then quality picker
      setShowSourcePicker(true);
    } else {
      // Browser: show quality picker directly (uses getDisplayMedia)
      setShowScreenSharePicker(true);
    }
  };

  // Handle native source selection → open quality picker with source ID
  const handleSourceSelected = (sourceId: string) => {
    setShowSourcePicker(false);
    setSelectedSourceId(sourceId);
    setShowScreenSharePicker(true);
  };

  // Keyboard shortcuts (only when in voice)
  const handleKeyDown = (e: KeyboardEvent) => {
    // Only handle shortcuts when connected to voice
    if (!voiceState.channelId) return;

    // Ctrl+Shift+M or Cmd+Shift+M: Toggle mute
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "M") {
      e.preventDefault();
      toggleMute();
    }

    // Ctrl+Shift+D or Cmd+Shift+D: Toggle deafen
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "D") {
      e.preventDefault();
      toggleDeafen();
    }
  };

  // Register keyboard listener
  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener("keydown", handleKeyDown);
  });

  // Drag handlers - optimized for performance
  let rafId: number | null = null;

  const handleMouseDown = (e: MouseEvent) => {
    // Don't start drag if clicking on a button
    if ((e.target as HTMLElement).closest("button")) return;

    setIsDragging(true);
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setDragOffset({
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
    });
    e.preventDefault(); // Prevent text selection
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (!isDragging() || !containerRef) return;

    // Use requestAnimationFrame for smooth updates
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
    }

    rafId = requestAnimationFrame(() => {
      const newX = e.clientX - dragOffset().x;
      const newY = e.clientY - dragOffset().y;

      // Get actual element dimensions
      const rect = containerRef!.getBoundingClientRect();
      const elementWidth = rect.width;
      const elementHeight = rect.height;

      // Keep fully within viewport bounds
      const maxX = window.innerWidth - elementWidth;
      const maxY = window.innerHeight - elementHeight;

      setPosition({
        x: Math.max(0, Math.min(newX, maxX)),
        y: Math.max(0, Math.min(newY, maxY)),
      });
    });
  };

  const handleMouseUp = () => {
    setIsDragging(false);
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  };

  // Add mouse move/up listeners when dragging
  createEffect(() => {
    if (isDragging()) {
      window.addEventListener("mousemove", handleMouseMove, { passive: false });
      window.addEventListener("mouseup", handleMouseUp);
    } else {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    }
  });

  onCleanup(() => {
    window.removeEventListener("mousemove", handleMouseMove);
    window.removeEventListener("mouseup", handleMouseUp);
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
    }
  });

  return (
    <div
      ref={containerRef}
      class="flex items-center gap-4 px-6 py-3 bg-black/60 backdrop-blur-md border rounded-full shadow-2xl relative select-none"
      classList={{
        "border-accent-primary/30": !voiceState.speaking,
        "border-accent-primary border-2 shadow-[0_0_20px_rgba(136,192,208,0.4)]":
          voiceState.speaking,
        "cursor-move": !isDragging(),
        "cursor-grabbing": isDragging(),
        "transition-none": isDragging(), // Disable transitions while dragging for better performance
        "transition-all duration-300": !isDragging(),
      }}
      style={{
        position: "fixed",
        left: `${position().x}px`,
        top: `${position().y}px`,
        "z-index": "100",
        transform: isDragging() ? "scale(1.02)" : "scale(1)", // Slight scale feedback when dragging
      }}
      onMouseDown={handleMouseDown}
    >
      {/* Drag Handle */}
      <div class="text-text-secondary/60 hover:text-text-secondary transition-colors cursor-grab active:cursor-grabbing">
        <GripVertical class="w-4 h-4" />
      </div>

      {/* Connection Status Indicator */}
      <div class="flex items-center gap-2">
        {/* Connection dot - subtle glow when idle, pulses when speaking
             Note: shadow RGBA is approximated from focused-hybrid theme's accent-success */}
        <div
          class="w-2.5 h-2.5 bg-accent-success rounded-full shadow-[0_0_4px_rgba(163,190,140,0.6)]"
          classList={{
            "animate-pulse": voiceState.speaking,
          }}
        />

        <span class="text-text-primary text-sm font-medium">
          {channelName()}
        </span>
      </div>

      {/* Timer */}
      <div class="text-text-secondary text-sm font-mono">{elapsedTime()}</div>

      {/* Quality Indicator */}
      <div
        class="relative"
        onMouseEnter={() => setShowQualityTooltip(true)}
        onMouseLeave={() => setShowQualityTooltip(false)}
      >
        <QualityIndicator metrics={getLocalMetrics()} mode={"circle"} />
        <Show
          when={showQualityTooltip() && typeof getLocalMetrics() === "object"}
        >
          <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 z-50">
            <QualityTooltip metrics={getLocalMetrics() as ConnectionMetrics} />
          </div>
        </Show>
      </div>

      {/* Divider */}
      <div class="w-px h-6 bg-white/20" />

      {/* Control Buttons */}
      <div class="flex items-center gap-2">
        {/* Mute/Unmute */}
        <button
          class="w-10 h-10 flex items-center justify-center rounded-full transition-all duration-200 hover:bg-white/10 relative"
          classList={{
            "text-accent-primary bg-accent-primary/20":
              !voiceState.muted && voiceState.speaking,
            "text-text-primary": !voiceState.muted && !voiceState.speaking,
            "text-accent-danger": voiceState.muted,
            "bg-accent-danger/20": voiceState.muted,
          }}
          onClick={toggleMute}
          title={
            voiceState.muted ? "Unmute (Ctrl+Shift+M)" : "Mute (Ctrl+Shift+M)"
          }
        >
          <Show when={!voiceState.muted} fallback={<MicOff class="w-5 h-5" />}>
            <Mic
              class="w-5 h-5 transition-all duration-200"
              classList={{
                "drop-shadow-[0_0_8px_rgba(136,192,208,0.8)]":
                  voiceState.speaking,
              }}
            />
          </Show>
        </button>

        {/* Deafen/Undeafen */}
        <button
          class="w-10 h-10 flex items-center justify-center rounded-full transition-all duration-200 hover:bg-white/10"
          classList={{
            "text-text-primary": !voiceState.deafened,
            "text-accent-danger": voiceState.deafened,
            "bg-accent-danger/20": voiceState.deafened,
          }}
          onClick={toggleDeafen}
          title={
            voiceState.deafened
              ? "Undeafen (Ctrl+Shift+D)"
              : "Deafen (Ctrl+Shift+D)"
          }
        >
          <Headphones
            class="w-5 h-5"
            classList={{ "opacity-50": voiceState.deafened }}
          />
        </button>

        {/* Screen Share */}
        <button
          class="w-10 h-10 flex items-center justify-center rounded-full transition-all duration-200 hover:bg-white/10"
          classList={{
            "text-accent-primary bg-accent-primary/20":
              voiceState.screenSharing,
            "text-text-primary": !voiceState.screenSharing,
          }}
          onClick={toggleScreenShare}
          title={
            voiceState.screenSharing ? "Stop Screen Share" : "Share Screen"
          }
        >
          <Monitor
            class="w-5 h-5"
            classList={{
              "drop-shadow-[0_0_8px_rgba(136,192,208,0.8)]":
                voiceState.screenSharing,
            }}
          />
        </button>

        {/* Settings */}
        <button
          class="w-10 h-10 flex items-center justify-center rounded-full transition-all duration-200 hover:bg-white/10 text-text-primary"
          onClick={() => setShowSettings(true)}
          title="Audio Settings"
        >
          <Settings class="w-5 h-5" />
        </button>

        {/* Divider */}
        <div class="w-px h-6 bg-white/20" />

        {/* Disconnect */}
        <button
          // Note: hover shadow RGBA is approximated from focused-hybrid theme's accent-danger
          class="w-10 h-10 flex items-center justify-center rounded-full bg-accent-danger/40 text-white transition-all duration-200 hover:bg-accent-danger hover:text-white hover:shadow-[0_0_12px_rgba(191,97,106,0.5)]"
          onClick={disconnect}
          title="Disconnect"
        >
          <PhoneOff class="w-5 h-5" />
        </button>
      </div>

      {/* Audio Settings Modal */}
      <Show when={showSettings()}>
        <LazyErrorBoundary name="AudioDeviceSettings">
          <Suspense fallback={<ModalFallback />}>
            <AudioDeviceSettings
              onClose={() => setShowSettings(false)}
              parentPosition={position()}
            />
          </Suspense>
        </LazyErrorBoundary>
      </Show>

      {/* Native Source Picker (Tauri only) */}
      <Show when={showSourcePicker()}>
        <LazyErrorBoundary name="ScreenShareSourcePicker">
          <Suspense fallback={<ModalFallback />}>
            <ScreenShareSourcePicker
              onSelect={handleSourceSelected}
              onClose={() => setShowSourcePicker(false)}
            />
          </Suspense>
        </LazyErrorBoundary>
      </Show>

      {/* Screen Share Quality Picker */}
      <Show when={showScreenSharePicker()}>
        <LazyErrorBoundary name="ScreenShareQualityPicker">
          <Suspense fallback={<ModalFallback />}>
            <ScreenShareQualityPicker
              sourceId={selectedSourceId()}
              onClose={() => {
                setShowScreenSharePicker(false);
                setSelectedSourceId(undefined);
              }}
            />
          </Suspense>
        </LazyErrorBoundary>
      </Show>
    </div>
  );
};

export default VoiceIsland;
