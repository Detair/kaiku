/**
 * Sound Settings Store
 *
 * Manages notification sound preferences with localStorage persistence.
 */

import { createSignal } from "solid-js";
import { currentUser } from "./auth";

// ============================================================================
// Types
// ============================================================================

export type SoundOption = "default" | "subtle" | "ping" | "chime" | "bell";
export type NotificationLevel = "all" | "mentions" | "none";

export interface QuietHoursSettings {
  /** Whether quiet hours are enabled */
  enabled: boolean;
  /** Start time in 24h format (e.g., "22:00") */
  startTime: string;
  /** End time in 24h format (e.g., "08:00") */
  endTime: string;
}

export interface SoundSettings {
  /** Master on/off for notification sounds */
  enabled: boolean;
  /** Volume level 0-100 */
  volume: number;
  /** Selected notification sound */
  selectedSound: SoundOption;
  /** Quiet hours / Do Not Disturb settings */
  quietHours: QuietHoursSettings;
}

export interface ChannelNotificationSettings {
  [channelId: string]: NotificationLevel;
}

// ============================================================================
// Storage Keys
// ============================================================================

const SOUND_SETTINGS_KEY = "canis:sound:settings";
const CHANNEL_SETTINGS_KEY = "canis:sound:channels";

// ============================================================================
// Defaults
// ============================================================================

const defaultQuietHours: QuietHoursSettings = {
  enabled: false,
  startTime: "22:00",
  endTime: "08:00",
};

const defaultSoundSettings: SoundSettings = {
  enabled: true,
  volume: 80,
  selectedSound: "default",
  quietHours: defaultQuietHours,
};

// ============================================================================
// Load Functions
// ============================================================================

function loadSoundSettings(): SoundSettings {
  if (typeof localStorage === "undefined") return defaultSoundSettings;
  const stored = localStorage.getItem(SOUND_SETTINGS_KEY);
  if (!stored) return defaultSoundSettings;
  try {
    const parsed = JSON.parse(stored);
    // Deep merge for nested quietHours to ensure backward compatibility
    return {
      ...defaultSoundSettings,
      ...parsed,
      quietHours: {
        ...defaultQuietHours,
        ...(parsed.quietHours ?? {}),
      },
    };
  } catch {
    return defaultSoundSettings;
  }
}

function loadChannelSettings(): ChannelNotificationSettings {
  if (typeof localStorage === "undefined") return {};
  const stored = localStorage.getItem(CHANNEL_SETTINGS_KEY);
  if (!stored) return {};
  try {
    return JSON.parse(stored);
  } catch {
    return {};
  }
}

// ============================================================================
// Signals
// ============================================================================

const [soundSettings, setSoundSettings] = createSignal<SoundSettings>(
  loadSoundSettings()
);

const [channelNotificationSettings, setChannelNotificationSettings] =
  createSignal<ChannelNotificationSettings>(loadChannelSettings());

// ============================================================================
// Sound Settings Functions
// ============================================================================

export function getSoundEnabled(): boolean {
  return soundSettings().enabled;
}

export function setSoundEnabled(enabled: boolean): void {
  const updated = { ...soundSettings(), enabled };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

export function getSoundVolume(): number {
  return soundSettings().volume;
}

export function setSoundVolume(volume: number): void {
  const clamped = Math.max(0, Math.min(100, volume));
  const updated = { ...soundSettings(), volume: clamped };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

export function getSelectedSound(): SoundOption {
  return soundSettings().selectedSound;
}

export function setSelectedSound(sound: SoundOption): void {
  const updated = { ...soundSettings(), selectedSound: sound };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

// ============================================================================
// Quiet Hours Functions
// ============================================================================

export function getQuietHours(): QuietHoursSettings {
  return soundSettings().quietHours;
}

export function setQuietHoursEnabled(enabled: boolean): void {
  const updated = {
    ...soundSettings(),
    quietHours: { ...soundSettings().quietHours, enabled },
  };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

export function setQuietHoursTime(startTime: string, endTime: string): void {
  const updated = {
    ...soundSettings(),
    quietHours: { ...soundSettings().quietHours, startTime, endTime },
  };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

/**
 * Check if current time is within quiet hours.
 * Handles overnight ranges (e.g., 22:00 to 08:00).
 */
export function isWithinQuietHours(): boolean {
  const { enabled, startTime, endTime } = soundSettings().quietHours;
  if (!enabled) return false;

  const now = new Date();
  const currentMinutes = now.getHours() * 60 + now.getMinutes();

  const [startHour, startMin] = startTime.split(":").map(Number);
  const [endHour, endMin] = endTime.split(":").map(Number);

  const startMinutes = startHour * 60 + startMin;
  const endMinutes = endHour * 60 + endMin;

  // Handle overnight range (e.g., 22:00 to 08:00)
  if (startMinutes > endMinutes) {
    // Current time is after start OR before end
    return currentMinutes >= startMinutes || currentMinutes < endMinutes;
  }

  // Normal range (e.g., 09:00 to 17:00)
  return currentMinutes >= startMinutes && currentMinutes < endMinutes;
}

/**
 * Check if Do Not Disturb is active.
 * DND is active when:
 * - User status is "busy"
 * - Quiet hours are currently active
 */
export function isDndActive(): boolean {
  const user = currentUser();
  if (user?.status === "busy") return true;
  return isWithinQuietHours();
}

// ============================================================================
// Channel Notification Functions
// ============================================================================

/**
 * Get notification level for a channel.
 * Default is "mentions" for channels, "all" for DMs.
 */
export function getChannelNotificationLevel(
  channelId: string,
  isDm: boolean = false
): NotificationLevel {
  const settings = channelNotificationSettings();
  return settings[channelId] ?? (isDm ? "all" : "mentions");
}

export function setChannelNotificationLevel(
  channelId: string,
  level: NotificationLevel
): void {
  const updated = { ...channelNotificationSettings(), [channelId]: level };
  setChannelNotificationSettings(updated);
  localStorage.setItem(CHANNEL_SETTINGS_KEY, JSON.stringify(updated));
}

/**
 * Check if a channel is muted (notification level = "none").
 */
export function isChannelMuted(channelId: string): boolean {
  return getChannelNotificationLevel(channelId) === "none";
}

// ============================================================================
// Exports
// ============================================================================

export { soundSettings, channelNotificationSettings };
