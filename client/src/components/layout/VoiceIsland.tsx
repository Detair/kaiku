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
 *
 * Controls:
 * - Mute/Unmute (Ctrl+Shift+M)
 * - Deafen/Undeafen (Ctrl+Shift+D)
 * - Audio Settings (opens AudioDeviceSettings modal)
 * - Screen Share (@phase4 - currently disabled)
 * - Disconnect (red button)
 *
 * Features:
 * - Global keyboard shortcuts for mute/deafen
 * - Real-time elapsed time with formatElapsedTime utility
 * - Decoupled channel name resolution via getVoiceChannelInfo()
 */

import { Component, createSignal, createEffect, onMount, onCleanup, Show } from "solid-js";
import { Mic, MicOff, Headphones, Monitor, PhoneOff, Settings, GripVertical } from "lucide-solid";
import { voiceState, setMute, setDeafen, leaveVoice, getVoiceChannelInfo, getLocalMetrics } from "@/stores/voice";
import { formatElapsedTime } from "@/lib/utils";
import AudioDeviceSettings from "@/components/voice/AudioDeviceSettings";
import { QualityIndicator } from "@/components/voice/QualityIndicator";
import { QualityTooltip } from "@/components/voice/QualityTooltip";
import type { ConnectionMetrics } from "@/lib/webrtc/types";

const VoiceIsland: Component = () => {
  const [elapsedTime, setElapsedTime] = createSignal<string>("00:00");
  const [showSettings, setShowSettings] = createSignal(false);
  const [showQualityTooltip, setShowQualityTooltip] = createSignal(false);

  // Draggable state - default to top-center
  const getInitialPosition = () => {
    // Estimate VoiceIsland width (~450px with all buttons)
    const estimatedWidth = 450;
    const centerX = (window.innerWidth - estimatedWidth) / 2;
    return {
      x: Math.max(0, centerX), // Ensure not negative
      y: 20 // 20px from top
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
        y: Math.max(0, Math.min(currentPos.y, maxY))
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
      y: e.clientY - rect.top
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
        y: Math.max(0, Math.min(newY, maxY))
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
        "border-accent-primary border-2 shadow-[0_0_20px_rgba(136,192,208,0.4)]": voiceState.speaking,
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
      <div class="text-text-secondary/40 hover:text-text-secondary transition-colors cursor-grab active:cursor-grabbing">
        <GripVertical class="w-4 h-4" />
      </div>

      {/* Connection Status Indicator */}
      <div class="flex items-center gap-2">
        {/* Connection dot - only pulses when speaking */}
        <div
          class="w-2 h-2 bg-accent-primary rounded-full"
          classList={{
            "animate-pulse": voiceState.speaking,
          }}
        />

        <span class="text-text-primary text-sm font-medium">{channelName()}</span>
      </div>

      {/* Timer */}
      <div class="text-text-secondary text-sm font-mono">{elapsedTime()}</div>

      {/* Quality Indicator */}
      <div
        class="relative"
        onMouseEnter={() => setShowQualityTooltip(true)}
        onMouseLeave={() => setShowQualityTooltip(false)}
      >
        <QualityIndicator
          metrics={getLocalMetrics()}
          mode={'circle'}
        />
        <Show when={showQualityTooltip() && typeof getLocalMetrics() === 'object'}>
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
            "text-accent-primary bg-accent-primary/20": !voiceState.muted && voiceState.speaking,
            "text-text-primary": !voiceState.muted && !voiceState.speaking,
            "text-accent-danger": voiceState.muted,
            "bg-accent-danger/20": voiceState.muted,
          }}
          onClick={toggleMute}
          title={voiceState.muted ? "Unmute (Ctrl+Shift+M)" : "Mute (Ctrl+Shift+M)"}
        >
          <Show when={!voiceState.muted} fallback={<MicOff class="w-5 h-5" />}>
            <Mic
              class="w-5 h-5 transition-all duration-200"
              classList={{
                "drop-shadow-[0_0_8px_rgba(136,192,208,0.8)]": voiceState.speaking,
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
          title={voiceState.deafened ? "Undeafen (Ctrl+Shift+D)" : "Deafen (Ctrl+Shift+D)"}
        >
          <Headphones class="w-5 h-5" classList={{ "opacity-50": voiceState.deafened }} />
        </button>

        {/* Screen Share - @phase4 */}
        <button
          class="w-10 h-10 flex items-center justify-center rounded-full transition-all duration-200 hover:bg-white/10 text-text-primary opacity-50 cursor-not-allowed"
          title="Screen Share (Phase 4)"
          disabled
        >
          <Monitor class="w-5 h-5" />
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
          class="w-10 h-10 flex items-center justify-center rounded-full bg-accent-danger/20 text-accent-danger transition-all duration-200 hover:bg-accent-danger hover:text-white"
          onClick={disconnect}
          title="Disconnect"
        >
          <PhoneOff class="w-5 h-5" />
        </button>
      </div>

      {/* Audio Settings Modal */}
      <Show when={showSettings()}>
        <AudioDeviceSettings
          onClose={() => setShowSettings(false)}
          parentPosition={position()}
        />
      </Show>
    </div>
  );
};

export default VoiceIsland;
