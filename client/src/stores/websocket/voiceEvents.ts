/**
 * Voice-related WebSocket event handlers: SDP exchange, ICE candidates,
 * participant state, room state, stats, and simulcast layer changes.
 */

import * as tauri from "@/lib/tauri";
import type { ScreenShareServerInfo, VoiceParticipant, WebcamServerInfo } from "@/lib/types";

export async function handleVoicePublisherAnswer(channelId: string, sdp: string): Promise<void> {
  try {
    const { getVoiceAdapter } = await import("@/lib/webrtc");
    const adapter = getVoiceAdapter();

    if (!adapter) {
      console.error("[WebSocket] No voice adapter available for publisher answer");
      return;
    }

    const result = await adapter.handlePublisherAnswer(channelId, sdp);
    if (!result.ok) {
      console.error("[WS] handlePublisherAnswer failed:", result.error);
    }
  } catch (err) {
    console.error("Error handling publisher answer:", err);
  }
}

export async function handleVoiceSubscriberOffer(channelId: string, sdp: string): Promise<void> {
  try {
    const { getVoiceAdapter } = await import("@/lib/webrtc");
    const adapter = getVoiceAdapter();

    if (!adapter) {
      console.error("[WebSocket] No voice adapter available for subscriber offer");
      return;
    }

    const result = await adapter.handleSubscriberOffer(channelId, sdp);
    if (!result.ok) {
      console.error("[WS] handleSubscriberOffer failed:", result.error);
      return;
    }

    // Send subscriber answer back to server
    await tauri.wsSend({
      type: "voice_subscriber_answer",
      channel_id: channelId,
      sdp: result.value,
    });
    console.log("[WebSocket] Subscriber answer sent successfully");
  } catch (err) {
    console.error("Error handling subscriber offer:", err);
  }
}

export async function handleVoiceIceCandidate(
  channelId: string,
  candidate: string,
  pcType: string = "publisher",
): Promise<void> {
  const startTime = performance.now();

  try {
    // Use getVoiceAdapter() to avoid dynamic import overhead (critical for ICE timing)
    const { getVoiceAdapter } = await import("@/lib/webrtc");
    const adapter = getVoiceAdapter();

    if (!adapter) {
      console.warn("[WebSocket] No voice adapter available for ICE candidate");
      return;
    }

    const result = await adapter.handleIceCandidate(channelId, candidate, pcType);

    const elapsed = performance.now() - startTime;
    console.log(
      `[WebSocket] ICE candidate (${pcType}) processed in ${elapsed.toFixed(2)}ms`,
    );

    if (!result.ok) {
      console.error(`Failed to handle ICE candidate (${pcType}):`, result.error);
    }
  } catch (err) {
    console.error("Error handling ICE candidate:", err);
  }
}

export async function handleVoiceUserJoined(
  channelId: string,
  userId: string,
  username: string,
  displayName: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        state.participants[userId] = {
          user_id: userId,
          username: username,
          display_name: displayName,
          muted: false,
          speaking: false,
          screen_sharing: false,
        };
      }),
    );
  }
}

export async function handleVoiceUserLeft(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        delete state.participants[userId];
      }),
    );
  }
}

export async function handleVoiceUserMuted(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = true;
        }
      }),
    );
  }
}

export async function handleVoiceUserUnmuted(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = false;
        }
      }),
    );
  }
}

export async function handleVoiceRoomState(
  channelId: string,
  participants: VoiceParticipant[],
  screenShares?: ScreenShareServerInfo[],
  webcams?: WebcamServerInfo[],
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        state.participants = {};
        for (const p of participants) {
          state.participants[p.user_id] = p;
        }
        state.screenShares = screenShares ?? [];
        state.webcams = webcams ?? [];
      }),
    );
  }
}

export async function handleVoiceUserStatsEvent(event: {
  channel_id: string;
  user_id: string;
  latency: number;
  packet_loss: number;
  jitter: number;
  quality: number;
}): Promise<void> {
  const { handleVoiceUserStats } = await import("@/stores/voice");
  handleVoiceUserStats(event);
}

export async function handleVoiceLayerChanged(event: {
  source_user_id: string;
  track_source: string;
  active_layer: "high" | "medium" | "low";
}): Promise<void> {
  const { handleLayerChanged } = await import("@/stores/simulcastLayers");
  handleLayerChanged(
    event.source_user_id,
    event.track_source,
    event.active_layer,
  );
}
