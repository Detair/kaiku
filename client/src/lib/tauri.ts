/**
 * Tauri Command Wrappers
 * Type-safe wrappers for Tauri commands
 */

import { invoke } from "@tauri-apps/api/core";
import type { User, Channel, Message, AppSettings } from "./types";

// Re-export types for convenience
export type { User, Channel, Message, AppSettings };

// Auth Commands

export async function login(
  serverUrl: string,
  username: string,
  password: string
): Promise<User> {
  return invoke("login", {
    request: { server_url: serverUrl, username, password },
  });
}

export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string
): Promise<User> {
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

export async function logout(): Promise<void> {
  return invoke("logout");
}

export async function getCurrentUser(): Promise<User | null> {
  return invoke("get_current_user");
}

// Chat Commands

export async function getChannels(): Promise<Channel[]> {
  return invoke("get_channels");
}

export async function getMessages(
  channelId: string,
  before?: string,
  limit?: number
): Promise<Message[]> {
  return invoke("get_messages", { channelId, before, limit });
}

export async function sendMessage(
  channelId: string,
  content: string
): Promise<Message> {
  return invoke("send_message", { channelId, content });
}

// Voice Commands

export async function joinVoice(channelId: string): Promise<void> {
  return invoke("join_voice", { channelId });
}

export async function leaveVoice(): Promise<void> {
  return invoke("leave_voice");
}

export async function setMute(muted: boolean): Promise<void> {
  return invoke("set_mute", { muted });
}

export async function setDeafen(deafened: boolean): Promise<void> {
  return invoke("set_deafen", { deafened });
}

// Settings Commands

export async function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  return invoke("update_settings", { settings });
}
