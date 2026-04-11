/**
 * Shared primitive types and constants used across domain modules.
 */

// Theme Types (canonical source of truth for theme names)

/** All available theme identifiers. Add new themes here. */
export const THEME_NAMES = [
  "focused-hybrid",
  "solarized-dark",
  "solarized-light",
  "pixel-cozy",
] as const;

/** Valid theme name identifier. Derived from THEME_NAMES array. */
export type ThemeName = (typeof THEME_NAMES)[number];

// User Types

export type UserStatus = "online" | "idle" | "dnd" | "invisible" | "offline";

// Quality and Status Indicator Types (for accessibility shapes)

export type QualityLevel = "good" | "warning" | "poor" | "unknown";

export type StatusShape = "circle" | "triangle" | "hexagon" | "empty-circle";

export const STATUS_SHAPES: Record<QualityLevel, StatusShape> = {
  good: "circle",
  warning: "triangle",
  poor: "hexagon",
  unknown: "empty-circle",
};

export const STATUS_COLORS = {
  good: "#23a55a",
  warning: "#f0b232",
  poor: "#f23f43",
  unknown: "#80848e",
  streaming: "#593695",
} as const;
