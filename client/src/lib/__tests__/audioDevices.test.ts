import { describe, expect, it } from "vitest";
import { hasHiddenDevices, visibleDevices } from "../audioDevices";
import type { AudioDevice } from "../webrtc/types";

const devices: AudioDevice[] = [
  { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
  { deviceId: "alsa:sysdefault:CARD=USB", label: "Scarlett 2i2 USB", isDefault: false, curated: true },
  {
    deviceId: "alsa:iec958:CARD=USB,DEV=0",
    label: "Scarlett 2i2 USB — IEC958 (S/PDIF) Digital Audio Output [iec958:CARD=USB,DEV=0]",
    isDefault: false,
    curated: false,
  },
  { deviceId: "alsa:eq", label: "Equalizer [eq]", isDefault: false, curated: false },
];

const ids = (list: AudioDevice[]) => list.map((d) => d.deviceId);

describe("visibleDevices", () => {
  it("hides non-curated devices by default", () => {
    expect(ids(visibleDevices(devices, false))).toEqual([
      "alsa:default",
      "alsa:sysdefault:CARD=USB",
    ]);
  });

  it("keeps the selected device listed while hidden", () => {
    expect(ids(visibleDevices(devices, false, "alsa:eq"))).toEqual([
      "alsa:default",
      "alsa:sysdefault:CARD=USB",
      "alsa:eq",
    ]);
  });

  it("shows everything with the option on", () => {
    expect(ids(visibleDevices(devices, true, null))).toEqual(ids(devices));
  });

  it("treats devices without the flag as curated (browser, other platforms)", () => {
    const browser: AudioDevice[] = [{ deviceId: "abc", label: "Mic", isDefault: false }];
    expect(visibleDevices(browser, false)).toEqual(browser);
    expect(hasHiddenDevices(browser)).toBe(false);
  });

  it("detects hidden devices", () => {
    expect(hasHiddenDevices(devices)).toBe(true);
  });
});
