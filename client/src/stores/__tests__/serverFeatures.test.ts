import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));

import * as tauri from "@/lib/tauri";
import {
  isFeatureEnabled,
  isFeatureOff,
  isPermissionVisible,
  loadServerFeatures,
  resetServerFeatures,
} from "../serverFeatures";

const settings = (features?: Record<string, boolean>) =>
  ({
    require_e2ee_setup: false,
    oidc_enabled: false,
    oidc_providers: [],
    auth_methods: {},
    registration_policy: "open",
    ...(features ? { features } : {}),
  }) as never;

const allOff = {
  forum_channels: false,
  announcement_channels: false,
  scheduled_events: false,
  membership_screening: false,
};

describe("serverFeatures", () => {
  beforeEach(() => {
    resetServerFeatures();
    vi.mocked(tauri.fetchServerSettings).mockReset();
  });

  it("is disabled until loaded", () => {
    expect(isFeatureEnabled("forum_channels")).toBe(false);
  });

  it("stays disabled when the settings request fails", async () => {
    vi.mocked(tauri.fetchServerSettings).mockRejectedValue(new Error("down"));
    await loadServerFeatures();
    expect(isFeatureEnabled("scheduled_events")).toBe(false);
  });

  it("missing features field means enabled", async () => {
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(settings());
    await loadServerFeatures();
    expect(isFeatureEnabled("forum_channels")).toBe(true);
    expect(isFeatureEnabled("announcement_channels")).toBe(true);
    expect(isFeatureEnabled("scheduled_events")).toBe(true);
    expect(isFeatureEnabled("membership_screening")).toBe(true);
  });

  it("explicit false disables", async () => {
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(
      settings({ ...allOff, scheduled_events: true }),
    );
    await loadServerFeatures();
    expect(isFeatureEnabled("scheduled_events")).toBe(true);
    expect(isFeatureEnabled("forum_channels")).toBe(false);
    expect(isFeatureEnabled("announcement_channels")).toBe(false);
    expect(isFeatureEnabled("membership_screening")).toBe(false);
  });

  it("hides the permission bits of disabled features", async () => {
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(
      settings({ ...allOff, forum_channels: true }),
    );
    await loadServerFeatures();
    expect(isPermissionVisible("MANAGE_POSTS")).toBe(true);
    expect(isPermissionVisible("SEND_ANNOUNCEMENTS")).toBe(false);
    expect(isPermissionVisible("MANAGE_EVENTS")).toBe(false);
    expect(isPermissionVisible("SEND_MESSAGES")).toBe(true);
  });

  it("unknown is not off; explicit off once loaded is", async () => {
    expect(isFeatureOff("forum_channels")).toBe(false);
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(settings(allOff));
    await loadServerFeatures();
    expect(isFeatureOff("forum_channels")).toBe(true);
    resetServerFeatures();
    expect(isFeatureOff("forum_channels")).toBe(false);
  });

  it("retries a failed load with backoff, and a reload after reconnect succeeds", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(tauri.fetchServerSettings)
        .mockRejectedValueOnce(new Error("down"))
        .mockResolvedValue(settings(allOff));
      await loadServerFeatures();
      expect(isFeatureEnabled("forum_channels")).toBe(false);
      expect(isFeatureOff("forum_channels")).toBe(false);
      await vi.advanceTimersByTimeAsync(2500);
      expect(isFeatureOff("forum_channels")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a reload (WebSocket ready) after a failure loads the flags", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(tauri.fetchServerSettings)
        .mockRejectedValueOnce(new Error("down"))
        .mockResolvedValue(settings());
      await loadServerFeatures();
      await loadServerFeatures();
      expect(isFeatureEnabled("scheduled_events")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("reset drops a pending retry", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(tauri.fetchServerSettings).mockRejectedValue(new Error("down"));
      await loadServerFeatures();
      resetServerFeatures();
      vi.mocked(tauri.fetchServerSettings).mockResolvedValue(settings(allOff));
      await vi.advanceTimersByTimeAsync(60000);
      expect(isFeatureOff("forum_channels")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
