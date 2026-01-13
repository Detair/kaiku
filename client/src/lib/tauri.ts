/**
 * Tauri Command Wrappers
 * Type-safe wrappers for Tauri commands
 * Falls back to HTTP API when running in browser
 */

import type { User, Channel, Message, AppSettings, Guild, GuildMember } from "./types";

// Re-export types for convenience
export type { User, Channel, Message, AppSettings, Guild, GuildMember };

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Auth response type from server
interface AuthResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  token_type: string;
}

// Browser state (when not in Tauri)
const browserState = {
  serverUrl: "http://localhost:8080",
  accessToken: null as string | null,
  refreshToken: null as string | null,
  tokenExpiresAt: null as number | null,
  refreshTimer: null as ReturnType<typeof setTimeout> | null,
};

// Initialize from localStorage if available
if (typeof localStorage !== "undefined") {
  browserState.serverUrl = localStorage.getItem("serverUrl") || browserState.serverUrl;
  browserState.accessToken = localStorage.getItem("accessToken");
  browserState.refreshToken = localStorage.getItem("refreshToken");
  const expiresAt = localStorage.getItem("tokenExpiresAt");
  browserState.tokenExpiresAt = expiresAt ? parseInt(expiresAt, 10) : null;
}

/**
 * Schedule automatic token refresh before expiration.
 * Refreshes 1 minute before the token expires.
 */
function scheduleTokenRefresh() {
  // Clear any existing timer
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }

  if (!browserState.tokenExpiresAt || !browserState.refreshToken) {
    return;
  }

  const now = Date.now();
  const expiresAt = browserState.tokenExpiresAt;
  // Refresh 60 seconds before expiration, but at least 10 seconds from now
  const refreshIn = Math.max(expiresAt - now - 60000, 10000);

  console.log(`[Auth] Scheduling token refresh in ${Math.round(refreshIn / 1000)}s`);

  browserState.refreshTimer = setTimeout(async () => {
    try {
      await refreshAccessToken();
    } catch (error) {
      console.error("[Auth] Auto-refresh failed:", error);
      // Token refresh failed - user will need to log in again
    }
  }, refreshIn);
}

/**
 * Refresh the access token using the refresh token.
 */
export async function refreshAccessToken(): Promise<boolean> {
  if (!browserState.refreshToken) {
    console.log("[Auth] No refresh token available");
    return false;
  }

  try {
    console.log("[Auth] Refreshing access token...");

    const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
    const response = await fetch(`${baseUrl}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: browserState.refreshToken }),
    });

    if (!response.ok) {
      console.error("[Auth] Token refresh failed:", response.status);
      // Clear tokens if refresh fails
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");
      return false;
    }

    const data: AuthResponse = await response.json();

    // Store new tokens
    browserState.accessToken = data.access_token;
    browserState.refreshToken = data.refresh_token;
    browserState.tokenExpiresAt = Date.now() + data.expires_in * 1000;

    localStorage.setItem("accessToken", data.access_token);
    localStorage.setItem("refreshToken", data.refresh_token);
    localStorage.setItem("tokenExpiresAt", browserState.tokenExpiresAt.toString());

    console.log("[Auth] Token refreshed successfully");

    // Schedule the next refresh
    scheduleTokenRefresh();

    return true;
  } catch (error) {
    console.error("[Auth] Token refresh error:", error);
    return false;
  }
}

// Start token refresh schedule on load if we have tokens
if (browserState.accessToken && browserState.refreshToken) {
  scheduleTokenRefresh();
}

// HTTP helper for browser mode
async function httpRequest<T>(
  method: string,
  path: string,
  body?: unknown
): Promise<T> {
  // Always read token fresh from localStorage to handle HMR reloads
  const token = browserState.accessToken || localStorage.getItem("accessToken");

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  // Remove trailing slash from serverUrl and ensure path starts with /
  const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
  const cleanPath = path.startsWith("/") ? path : `/${path}`;

  console.log(`[httpRequest] ${method} ${path}`, {
    hasToken: !!token,
    hasAuthHeader: !!headers["Authorization"],
    headers: JSON.stringify(headers),
  });

  const response = await fetch(`${baseUrl}${cleanPath}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ message: response.statusText }));
    throw new Error(error.message || error.error || "Request failed");
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) return null as T;
  return JSON.parse(text);
}

// Auth Commands

export async function login(
  serverUrl: string,
  username: string,
  password: string
): Promise<User> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("login", {
      request: { server_url: serverUrl, username, password },
    });
  }

  // Browser mode
  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);

  const response = await httpRequest<AuthResponse>(
    "POST",
    "/auth/login",
    { username, password }
  );

  // Store all token data
  browserState.accessToken = response.access_token;
  browserState.refreshToken = response.refresh_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

  localStorage.setItem("accessToken", response.access_token);
  localStorage.setItem("refreshToken", response.refresh_token);
  localStorage.setItem("tokenExpiresAt", browserState.tokenExpiresAt.toString());

  // Schedule automatic token refresh
  scheduleTokenRefresh();

  console.log(`[Auth] Login successful, token expires in ${response.expires_in}s`);

  // Fetch user profile after login
  return await httpRequest<User>("GET", "/auth/me");
}

export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string
): Promise<User> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("register", {
      request: {
        server_url: serverUrl,
        username,
        email,
        password,
        display_name: displayName,
      },
    });
  }

  // Browser mode
  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);

  const response = await httpRequest<AuthResponse>(
    "POST",
    "/auth/register",
    { username, password, email, display_name: displayName }
  );

  // Store all token data
  browserState.accessToken = response.access_token;
  browserState.refreshToken = response.refresh_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

  localStorage.setItem("accessToken", response.access_token);
  localStorage.setItem("refreshToken", response.refresh_token);
  localStorage.setItem("tokenExpiresAt", browserState.tokenExpiresAt.toString());

  // Schedule automatic token refresh
  scheduleTokenRefresh();

  console.log(`[Auth] Registration successful, token expires in ${response.expires_in}s`);

  // Fetch user profile after registration
  return await httpRequest<User>("GET", "/auth/me");
}

export async function logout(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("logout");
  }

  // Browser mode - clear all token state
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }

  browserState.accessToken = null;
  browserState.refreshToken = null;
  browserState.tokenExpiresAt = null;

  localStorage.removeItem("accessToken");
  localStorage.removeItem("refreshToken");
  localStorage.removeItem("tokenExpiresAt");

  console.log("[Auth] Logged out, tokens cleared");
}

export async function getCurrentUser(): Promise<User | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_current_user");
  }

  // Browser mode - check if we have a token
  if (!browserState.accessToken) {
    // Try to refresh if we have a refresh token
    if (browserState.refreshToken) {
      const refreshed = await refreshAccessToken();
      if (!refreshed) {
        return null;
      }
    } else {
      return null;
    }
  }

  try {
    return await httpRequest<User>("GET", "/auth/me");
  } catch {
    // Token invalid - try to refresh
    if (browserState.refreshToken) {
      const refreshed = await refreshAccessToken();
      if (refreshed) {
        try {
          return await httpRequest<User>("GET", "/auth/me");
        } catch {
          // Refresh didn't help, clear everything
        }
      }
    }

    // Clear all token state
    if (browserState.refreshTimer) {
      clearTimeout(browserState.refreshTimer);
      browserState.refreshTimer = null;
    }
    browserState.accessToken = null;
    browserState.refreshToken = null;
    browserState.tokenExpiresAt = null;
    localStorage.removeItem("accessToken");
    localStorage.removeItem("refreshToken");
    localStorage.removeItem("tokenExpiresAt");
    return null;
  }
}

// Chat Commands

export async function getChannels(): Promise<Channel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_channels");
  }

  return httpRequest<Channel[]>("GET", "/api/channels");
}

export async function createChannel(
  name: string,
  channelType: "text" | "voice",
  topic?: string
): Promise<Channel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_channel", { name, channelType, topic });
  }

  return httpRequest<Channel>("POST", "/api/channels", {
    name,
    channel_type: channelType,
    topic,
  });
}

export async function getMessages(
  channelId: string,
  before?: string,
  limit?: number
): Promise<Message[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_messages", { channelId, before, limit });
  }

  const params = new URLSearchParams();
  if (before) params.set("before", before);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<Message[]>(
    "GET",
    `/api/messages/channel/${channelId}${query ? `?${query}` : ""}`
  );
}

export async function sendMessage(
  channelId: string,
  content: string
): Promise<Message> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("send_message", { channelId, content });
  }

  return httpRequest<Message>("POST", `/api/messages/channel/${channelId}`, {
    content,
  });
}

export async function uploadFile(
  messageId: string,
  file: File
): Promise<any> {
  // For now, we use standard fetch for both Browser and Tauri
  // Tauri 2.0 supports fetch with proper configuration
  
  const token = browserState.accessToken || localStorage.getItem("accessToken");
  const headers: Record<string, string> = {};

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("message_id", messageId);
  formData.append("file", file);

  const baseUrl = (browserState.serverUrl || "http://localhost:8080").replace(/\/+$/, "");
  
  const response = await fetch(`${baseUrl}/api/messages/upload`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ message: response.statusText }));
    throw new Error(error.message || error.error || "Upload failed");
  }

  return response.json();
}

/**
 * Upload a file and create a message in one request.
 * Uses the combined endpoint that creates the message and attaches the file.
 */
export async function uploadMessageWithFile(
  channelId: string,
  file: File,
  content?: string
): Promise<Message> {
  const token = browserState.accessToken || localStorage.getItem("accessToken");
  const headers: Record<string, string> = {};

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("file", file);
  if (content) {
    formData.append("content", content);
  }

  const baseUrl = (browserState.serverUrl || "http://localhost:8080").replace(/\/+$/, "");

  const response = await fetch(`${baseUrl}/api/messages/channel/${channelId}/upload`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ message: response.statusText }));
    throw new Error(error.message || error.error || "Upload failed");
  }

  return response.json();
}

// Guild Commands

export async function getGuilds(): Promise<Guild[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guilds");
  }

  return httpRequest<Guild[]>("GET", "/api/guilds");
}

export async function getGuild(guildId: string): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild", { guildId });
  }

  return httpRequest<Guild>("GET", `/api/guilds/${guildId}`);
}

export async function createGuild(
  name: string,
  description?: string
): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild", { name, description });
  }

  return httpRequest<Guild>("POST", "/api/guilds", { name, description });
}

export async function updateGuild(
  guildId: string,
  name?: string,
  description?: string,
  icon_url?: string
): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild", { guildId, name, description, iconUrl: icon_url });
  }

  return httpRequest<Guild>("PATCH", `/api/guilds/${guildId}`, {
    name,
    description,
    icon_url,
  });
}

export async function deleteGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild", { guildId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}`);
}

export async function joinGuild(guildId: string, inviteCode: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_guild", { guildId, inviteCode });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/join`, {
    invite_code: inviteCode,
  });
}

export async function leaveGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_guild", { guildId });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/leave`);
}

export async function getGuildMembers(guildId: string): Promise<GuildMember[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_members", { guildId });
  }

  return httpRequest<GuildMember[]>("GET", `/api/guilds/${guildId}/members`);
}

export async function getGuildChannels(guildId: string): Promise<Channel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_channels", { guildId });
  }

  return httpRequest<Channel[]>("GET", `/api/guilds/${guildId}/channels`);
}

// Voice Commands (browser mode stubs - voice requires Tauri)

export async function joinVoice(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_voice", { channelId });
  }
  console.warn("Voice chat requires the native app");
}

export async function leaveVoice(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice");
  }
}

export async function setMute(muted: boolean): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_mute", { muted });
  }
}

export async function setDeafen(deafened: boolean): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_deafen", { deafened });
  }
}

// Settings Commands

export async function getSettings(): Promise<AppSettings> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_settings");
  }

  // Browser mode - return defaults
  return {
    audio: {
      input_device: null,
      output_device: null,
      input_volume: 100,
      output_volume: 100,
      noise_suppression: true,
      echo_cancellation: true,
    },
    voice: {
      push_to_talk: false,
      push_to_talk_key: null,
      voice_activity_detection: true,
      vad_threshold: 0.5,
    },
    theme: "dark",
    notifications_enabled: true,
  };
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_settings", { settings });
  }
  // Browser mode - no-op
}

// WebSocket Commands

export type ConnectionStatus =
  | { type: "disconnected" }
  | { type: "connecting" }
  | { type: "connected" }
  | { type: "reconnecting"; attempt: number };

// Browser WebSocket instance
let browserWs: WebSocket | null = null;
let browserWsStatus: ConnectionStatus = { type: "disconnected" };

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
  // Server expects token in query string
  const wsUrl = browserState.serverUrl.replace(/^http/, "ws") + "/ws?token=" + encodeURIComponent(browserState.accessToken);

  return new Promise((resolve, reject) => {
    browserWs = new WebSocket(wsUrl);

    browserWs.onopen = async () => {
      browserWsStatus = { type: "connected" };
      console.log("[WebSocket] Connected to server");

      // Re-initialize WebSocket event listeners
      try {
        const { reinitWebSocketListeners } = await import("@/stores/websocket");
        await reinitWebSocketListeners();
        console.log("[WebSocket] Event listeners reinitialized");
      } catch (err) {
        console.error("[WebSocket] Failed to reinitialize listeners:", err);
      }

      resolve();
    };

    browserWs.onerror = (err) => {
      browserWsStatus = { type: "disconnected" };
      console.error("[WebSocket] Connection error:", err);
      reject(err);
    };

    browserWs.onclose = () => {
      browserWsStatus = { type: "disconnected" };
      console.log("[WebSocket] Connection closed");
    };
  });
}

export async function wsDisconnect(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_disconnect");
  }

  browserWs?.close();
  browserWs = null;
  browserWsStatus = { type: "disconnected" };
}

export async function wsStatus(): Promise<ConnectionStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_status");
  }

  return browserWsStatus;
}

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

  browserWs?.send(JSON.stringify({ type: "unsubscribe", channel_id: channelId }));
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

  browserWs?.send(JSON.stringify({ type: "stop_typing", channel_id: channelId }));
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

export function getServerUrl(): string {
  return browserState.serverUrl;
}

/**
 * Get the current access token (for use in URLs that can't use headers).
 */
export function getAccessToken(): string | null {
  return browserState.accessToken || localStorage.getItem("accessToken");
}

/**
 * Send a WebSocket message (works in both browser and Tauri modes)
 */
export async function wsSend(message: any): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("ws_send", { message: JSON.stringify(message) });
  } else {
    if (!browserWs || browserWs.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket not connected. Current state: " + (browserWs ? browserWs.readyState : "null"));
    }
    browserWs.send(JSON.stringify(message));
  }
}
