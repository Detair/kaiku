/**
 * WebSocket commands: connect, subscribe, typing indicators, admin subscribe,
 * and media-event helpers (screen share, webcam).
 *
 * Browser mode stores the WebSocket instance in module-local state so other
 * domain modules (e.g. auth.ts presence updates) can send via the same socket.
 */

import { browserState, isTauri } from "./common";

// ============================================================================
// Connection state (browser mode)
// ============================================================================

export type ConnectionStatus =
  | { type: "disconnected" }
  | { type: "connecting" }
  | { type: "connected" }
  | { type: "reconnecting"; attempt: number };

// Browser WebSocket instance and status — module-local state so this file
// owns the socket while other modules read it via getBrowserWebSocket().
let browserWs: WebSocket | null = null;
let browserWsStatus: ConnectionStatus = { type: "disconnected" };

/** Lifecycle callbacks for the current browser socket (reconnect supervisor). */
export interface BrowserSocketHooks {
  /** The live socket closed; `intentional` is true only for wsDisconnect(). */
  onClose(intentional: boolean): void;
  /** Any inbound frame arrived (watchdog liveness). */
  onFrame(): void;
}

let socketHooks: BrowserSocketHooks | null = null;

export function setBrowserSocketHooks(hooks: BrowserSocketHooks): void {
  socketHooks = hooks;
}

// ============================================================================
// Connection lifecycle
// ============================================================================

export async function wsConnect(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_connect");
  }

  // Browser mode
  if (browserWs?.readyState === WebSocket.OPEN) return;

  if (!browserState.accessToken) {
    throw new Error("No access token available for WebSocket connection");
  }

  browserWsStatus = { type: "connecting" };
  // Server expects token in Sec-WebSocket-Protocol header.
  // Send both the token protocol and "access_token" so the server can echo
  // back "access_token" (which the browser accepts as a matching protocol).
  const wsUrl = browserState.serverUrl.replace(/^http/, "ws") + "/ws";
  const wsTokenProtocol = `access_token.${browserState.accessToken}`;

  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl, [wsTokenProtocol, "access_token"]);
    browserWs = ws;
    // Events of a socket that is no longer `browserWs` (replaced, or closed
    // via wsDisconnect) are ignored so a late close can't hit a newer socket.
    const isCurrent = () => browserWs === ws;

    ws.onopen = () => {
      if (!isCurrent()) {
        ws.close();
        reject(new Error("WebSocket connection was cancelled"));
        return;
      }
      browserWsStatus = { type: "connected" };
      console.log("[WebSocket] Connected to server");

      // Dispatch ws-connected event so waitForConnection() resolves in browser mode
      window.dispatchEvent(new Event("ws-connected"));

      resolve();
    };

    ws.addEventListener("message", () => {
      if (isCurrent()) socketHooks?.onFrame();
    });

    ws.onerror = (err) => {
      if (isCurrent()) browserWsStatus = { type: "disconnected" };
      console.error("[WebSocket] Connection error:", err);
      reject(err);
    };

    ws.onclose = () => {
      if (!isCurrent()) return;
      browserWs = null;
      browserWsStatus = { type: "disconnected" };
      console.log("[WebSocket] Connection closed");
      // No-op if the socket had opened; settles a handshake that closed early.
      reject(new Error("WebSocket closed before it opened"));
      socketHooks?.onClose(false);
    };
  });
}

/** Close the socket on purpose (logout, session end); never auto-reconnects. */
export async function wsDisconnect(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_disconnect");
  }

  const ws = browserWs;
  // Detach first: the socket's own onclose then sees it is no longer current.
  browserWs = null;
  browserWsStatus = { type: "disconnected" };
  if (ws) {
    ws.close();
    socketHooks?.onClose(true);
  }
}

export async function wsStatus(): Promise<ConnectionStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_status");
  }

  return browserWsStatus;
}

// ============================================================================
// Subscribe / unsubscribe / typing
// ============================================================================

export async function wsSubscribe(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_subscribe", { channelId });
  }

  browserWs?.send(JSON.stringify({ type: "subscribe", channel_id: channelId }));
}

export async function wsUnsubscribe(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_unsubscribe", { channelId });
  }

  browserWs?.send(
    JSON.stringify({ type: "unsubscribe", channel_id: channelId }),
  );
}

export async function wsTyping(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_typing", { channelId });
  }

  browserWs?.send(JSON.stringify({ type: "typing", channel_id: channelId }));
}

export async function wsStopTyping(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_stop_typing", { channelId });
  }

  browserWs?.send(
    JSON.stringify({ type: "stop_typing", channel_id: channelId }),
  );
}

export async function wsPing(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_ping");
  }

  browserWs?.send(JSON.stringify({ type: "ping" }));
}

// Export browser WebSocket for event handling
export function getBrowserWebSocket(): WebSocket | null {
  return isTauri ? null : browserWs;
}

/**
 * Send a WebSocket message (works in both browser and Tauri modes).
 */
export async function wsSend(message: any): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("ws_send", { message: JSON.stringify(message) });
  } else {
    if (!browserWs || browserWs.readyState !== WebSocket.OPEN) {
      throw new Error(
        "WebSocket not connected. Current state: " +
        (browserWs ? browserWs.readyState : "null"),
      );
    }
    browserWs.send(JSON.stringify(message));
  }
}

// ============================================================================
// Admin subscription
// ============================================================================

/**
 * Subscribe to admin events (requires elevated admin).
 */
export async function wsAdminSubscribe(): Promise<void> {
  await wsSend({ type: "admin_subscribe" });
}

/**
 * Unsubscribe from admin events.
 */
export async function wsAdminUnsubscribe(): Promise<void> {
  await wsSend({ type: "admin_unsubscribe" });
}

// ============================================================================
// Media: screen share and webcam signalling
// ============================================================================

/**
 * Start screen sharing in a voice channel (notifies server).
 */
export async function wsScreenShareStart(
  channelId: string,
  streamId: string,
  quality: "low" | "medium" | "high" | "premium",
  hasAudio: boolean,
  sourceLabel: string,
): Promise<void> {
  await wsSend({
    type: "voice_screen_share_start",
    channel_id: channelId,
    stream_id: streamId,
    quality,
    has_audio: hasAudio,
    source_label: sourceLabel,
  });
}

/**
 * Stop screen sharing in a voice channel (notifies server).
 */
export async function wsScreenShareStop(
  channelId: string,
  streamId: string,
): Promise<void> {
  await wsSend({
    type: "voice_screen_share_stop",
    channel_id: channelId,
    stream_id: streamId,
  });
}

/**
 * Start webcam in a voice channel (notifies server).
 */
export async function wsWebcamStart(
  channelId: string,
  quality: "low" | "medium" | "high" | "premium",
): Promise<void> {
  await wsSend({
    type: "voice_webcam_start",
    channel_id: channelId,
    quality,
  });
}

/**
 * Stop webcam in a voice channel (notifies server).
 */
export async function wsWebcamStop(channelId: string): Promise<void> {
  await wsSend({
    type: "voice_webcam_stop",
    channel_id: channelId,
  });
}
