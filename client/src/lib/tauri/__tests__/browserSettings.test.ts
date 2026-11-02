import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../common", async (orig) => ({
  ...(await orig<object>()),
  isTauri: () => false,
}));

import {
  getSettings,
  updateSettings,
  defaultSettings,
  BROWSER_SETTINGS_KEY,
} from "../settings";

describe("browser-mode settings persistence", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("returns defaults when nothing is stored", async () => {
    expect(await getSettings()).toEqual(defaultSettings());
  });

  it("persists audio and voice settings across a reload", async () => {
    const next = defaultSettings();
    next.audio.noise_suppression = false;
    next.audio.input_device = "mic-1";
    next.audio.output_device = "spk-2";
    next.audio.output_volume = 40;
    next.voice.push_to_talk = true;
    next.voice.push_to_talk_key = "Ctrl+KeyV";
    next.voice.vad_threshold = 0.2;
    await updateSettings(next);

    expect(await getSettings()).toEqual(next);
  });

  it("falls back to defaults on corrupt JSON", async () => {
    localStorage.setItem(BROWSER_SETTINGS_KEY, "{not json");
    expect(await getSettings()).toEqual(defaultSettings());
    localStorage.setItem(BROWSER_SETTINGS_KEY, "42");
    expect(await getSettings()).toEqual(defaultSettings());
  });

  it("merges partial values with defaults and drops wrong types", async () => {
    localStorage.setItem(
      BROWSER_SETTINGS_KEY,
      JSON.stringify({
        audio: { input_volume: 55, noise_suppression: "yes", extra: 1 },
        voice: { push_to_talk_key: 7, vad_threshold: 0.9 },
        theme: "neon",
      }),
    );
    const s = await getSettings();
    const d = defaultSettings();
    expect(s.audio.input_volume).toBe(55);
    expect(s.audio.noise_suppression).toBe(d.audio.noise_suppression);
    expect(s.voice.push_to_talk_key).toBeNull();
    expect(s.voice.vad_threshold).toBe(0.9);
    expect(s.theme).toBe("dark");
    expect("extra" in s.audio).toBe(false);
  });

  it("survives unavailable storage", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    await expect(updateSettings(defaultSettings())).resolves.toBeUndefined();
    expect(await getSettings()).toEqual(defaultSettings());
  });
});
