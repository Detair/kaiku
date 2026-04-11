/**
 * User preferences: display, focus mode, notifications, sound, etc.
 *
 * Synced across devices via the server.
 */

import type { ThemeName } from "./common";

// Display Preferences Types
export type DisplayMode = "dense" | "minimal" | "discord";
export type ReactionStyle = "bar" | "compact";

export interface DisplayPreferences {
  /** How status indicators are displayed (dense=full info, minimal=compact, discord=Discord-style) */
  indicator_mode: DisplayMode;
  /** Whether to show latency numbers on voice indicators */
  show_latency_numbers: boolean;
  /** How reactions are displayed on messages */
  reaction_style: ReactionStyle;
  /** Minutes of inactivity before user is marked as idle */
  idle_timeout_minutes: number;
}

export const DEFAULT_DISPLAY_PREFERENCES: DisplayPreferences = {
  indicator_mode: "dense",
  show_latency_numbers: true,
  reaction_style: "bar",
  idle_timeout_minutes: 5,
};

// Focus Mode Types

export type FocusTriggerCategory = "game" | "coding" | "listening" | "watching";

export type FocusSuppressionLevel = "all" | "except_mentions" | "except_dms";

export interface FocusMode {
  id: string;
  name: string;
  icon: string;
  builtin: boolean;
  trigger_categories: FocusTriggerCategory[] | null;
  auto_activate_enabled: boolean;
  suppression_level: FocusSuppressionLevel;
  vip_user_ids: string[];
  vip_channel_ids: string[];
  emergency_keywords: string[];
}

export interface FocusPreferences {
  modes: FocusMode[];
  auto_activate_global: boolean;
  custom_app_rules: Record<string, FocusTriggerCategory>;
}

export interface FocusState {
  active_mode_id: string | null;
  auto_activated: boolean;
  activated_at: string | null;
  triggering_category: FocusTriggerCategory | null;
}

export interface NotificationPreferences {
  os_enabled: boolean;
  show_content: boolean;
  flash_taskbar: boolean;
}

// User Preferences (synced across devices)
export interface UserPreferences {
  // Theme
  theme: ThemeName;

  // Sound settings
  sound: {
    enabled: boolean;
    volume: number; // 0-100
    sound_type: "default" | "subtle" | "ping" | "chime" | "bell";
    quiet_hours: {
      enabled: boolean;
      start_time: string; // "HH:MM" format
      end_time: string;
    };
  };

  // Connection display
  connection: {
    display_mode: "circle" | "number";
    show_notifications: boolean;
  };

  // Per-channel notification levels
  channel_notifications: Record<string, "all" | "mentions" | "muted">;

  // Home sidebar section collapse states
  home_sidebar: {
    collapsed: {
      unread: boolean;
      active_now: boolean;
      pending: boolean;
      pins: boolean;
    };
  };

  // Display preferences for UI customization
  display: DisplayPreferences;

  // Focus mode preferences
  focus: FocusPreferences;

  // Desktop notification preferences
  notifications: NotificationPreferences;

  // Onboarding completion flag
  onboarding_completed: boolean;
}

export interface PreferencesResponse {
  preferences: Partial<UserPreferences>;
  updated_at: string; // ISO timestamp
}

export interface StoredPreferences {
  data: UserPreferences;
  updated_at: string;
}
