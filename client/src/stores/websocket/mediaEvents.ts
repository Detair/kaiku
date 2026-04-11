/**
 * Screen share and webcam event handlers.
 */

import type { ServerEvent } from "@/lib/types";

// Screen share event handlers

export async function handleScreenShareStarted(event: Omit<Extract<ServerEvent, { type: "screen_share_started" }>, "type">): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share started:", event.user_id, event.stream_id);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Add to screen shares list
        state.screenShares.push({
          stream_id: event.stream_id,
          user_id: event.user_id,
          username: event.username,
          source_label: event.source_label,
          has_audio: event.has_audio,
          quality: event.quality,
          started_at: event.started_at ?? new Date().toISOString(),
        });

        // Update participant's screen_sharing flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].screen_sharing = true;
        }
      }),
    );
  }
}

export async function handleScreenShareStopped(event: Omit<Extract<ServerEvent, { type: "screen_share_stopped" }>, "type">): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share stopped:", event.user_id, event.stream_id, event.reason);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Remove the specific stream from screen shares list
        state.screenShares = state.screenShares.filter(
          (s) => s.stream_id !== event.stream_id,
        );

        // Update participant's screen_sharing flag only if they have no more shares
        const hasOtherShares = state.screenShares.some(
          (s) => s.user_id === event.user_id,
        );
        if (!hasOtherShares && state.participants[event.user_id]) {
          state.participants[event.user_id].screen_sharing = false;
        }

      }),
    );
  }
}

export async function handleScreenShareQualityChanged(
  event: Omit<Extract<ServerEvent, { type: "screen_share_quality_changed" }>, "type">,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log(
    "[WebSocket] Screen share quality changed:",
    event.user_id,
    event.stream_id,
    event.new_quality,
  );

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        const share = state.screenShares.find(
          (s) => s.stream_id === event.stream_id,
        );
        if (share) {
          share.quality = event.new_quality;
        }
      }),
    );
  }
}

// Webcam event handlers

export async function handleWebcamStarted(event: Omit<Extract<ServerEvent, { type: "webcam_started" }>, "type">): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Webcam started:", event.user_id);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Add to webcams list
        state.webcams.push({
          user_id: event.user_id,
          username: event.username,
          quality: event.quality,
        });

        // Update participant's webcam_active flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].webcam_active = true;
        }
      }),
    );
  }
}

export async function handleWebcamStopped(event: Omit<Extract<ServerEvent, { type: "webcam_stopped" }>, "type">): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Webcam stopped:", event.user_id, event.reason);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Remove from webcams list
        state.webcams = state.webcams.filter(
          (w) => w.user_id !== event.user_id,
        );

        // Update participant's webcam_active flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].webcam_active = false;
        }

        // If it was us, clear local state
        // (authState comparison not available here, so the voice store handles it via WS event)
      }),
    );
  }
}
