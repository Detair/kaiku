import { describe, expect, it } from "vitest";
import {
  hasHiddenDevices,
  preselectedDeviceId,
  systemDefaultDevice,
  visibleDevices,
} from "../audioDevices";
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

describe("preselectedDeviceId", () => {
  it("prefers the saved device, matched by id", () => {
    expect(preselectedDeviceId(devices, "alsa:sysdefault:CARD=USB")).toBe("alsa:sysdefault:CARD=USB");
  });

  it("does not match a saved value by label", () => {
    expect(preselectedDeviceId(devices, "Scarlett 2i2 USB")).toBe("alsa:default");
  });

  it("falls back to the system default entry, then the first device", () => {
    expect(preselectedDeviceId(devices, null)).toBe("alsa:default");
    expect(preselectedDeviceId(devices, "gone")).toBe("alsa:default");
    const noDefault = devices.filter((d) => !d.isDefault);
    expect(preselectedDeviceId(noDefault, undefined)).toBe("alsa:sysdefault:CARD=USB");
    expect(preselectedDeviceId([], "x")).toBe("");
  });

  it("finds the system default entry", () => {
    expect(systemDefaultDevice(devices)?.deviceId).toBe("alsa:default");
    expect(systemDefaultDevice(devices.slice(1))).toBeUndefined();
  });
});
