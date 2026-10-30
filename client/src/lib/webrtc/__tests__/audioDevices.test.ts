import { describe, it, expect, afterEach, vi } from "vitest";
import { BrowserVoiceAdapter } from "../browser";

function dev(kind: MediaDeviceKind, deviceId: string, label: string): MediaDeviceInfo {
  return { kind, deviceId, label, groupId: "", toJSON: () => ({}) } as MediaDeviceInfo;
}

function stubMediaDevices(devices: MediaDeviceInfo[]) {
  const getUserMedia = vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] });
  const enumerateDevices = vi.fn().mockResolvedValue(devices);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia, enumerateDevices } });
  return { getUserMedia, enumerateDevices };
}

describe("BrowserVoiceAdapter device enumeration", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("does not request the microphone when listing devices", async () => {
    const { getUserMedia, enumerateDevices } = stubMediaDevices([
      dev("audioinput", "a", "USB Mic"),
      dev("audiooutput", "b", "Speakers"),
    ]);
    const r = await new BrowserVoiceAdapter().getAudioDevices();
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(enumerateDevices).toHaveBeenCalled();
    expect(r.ok && r.value.labelsAvailable).toBe(true);
  });

  it("flags missing labels and drops id-less placeholders", async () => {
    const { getUserMedia } = stubMediaDevices([
      dev("audioinput", "", ""),
      dev("audiooutput", "", ""),
    ]);
    const r = await new BrowserVoiceAdapter().getAudioDevices();
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value.labelsAvailable).toBe(false);
      expect(r.value.inputs).toEqual([]);
    }
  });

  it("requests the microphone only via requestDeviceLabels", async () => {
    const { getUserMedia } = stubMediaDevices([dev("audioinput", "a", "USB Mic")]);
    const r = await new BrowserVoiceAdapter().requestDeviceLabels();
    expect(getUserMedia).toHaveBeenCalledTimes(1);
    expect(r.ok && r.value.inputs[0].label).toBe("USB Mic");
  });
});
