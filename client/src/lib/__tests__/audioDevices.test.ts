import { describe, expect, it } from "vitest";
import {
  findSelectedDevice,
  hasHiddenDevices,
  labelNamesSystemDefault,
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

describe("legacy device names", () => {
  const legacy: AudioDevice[] = [
    { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true, legacyName: "Default ALSA Output (currently PipeWire Media Server)" },
    { deviceId: "alsa:sysdefault:CARD=A", label: "USB Mic", isDefault: false, curated: true, legacyName: "USB Audio, USB Audio" },
    { deviceId: "alsa:sysdefault:CARD=B", label: "USB Mic", isDefault: false, curated: true, legacyName: "Webcam, USB Audio" },
    { deviceId: "alsa:front:CARD=B,DEV=0", label: "Webcam, USB Audio [front:CARD=B,DEV=0]", isDefault: false, curated: false, legacyName: "Webcam, USB Audio" },
    { deviceId: "alsa:eq", label: "Equalizer [eq]", isDefault: false, curated: false, legacyName: "Equalizer" },
  ];

  it("resolves a legacy saved name to the matching device", () => {
    expect(preselectedDeviceId(legacy, "Webcam, USB Audio")).toBe("alsa:sysdefault:CARD=B");
  });

  it("prefers a curated device for a legacy name, then any device", () => {
    expect(findSelectedDevice(legacy, "Webcam, USB Audio")?.deviceId).toBe("alsa:sysdefault:CARD=B");
    expect(preselectedDeviceId(legacy, "Equalizer")).toBe("alsa:eq");
  });

  it("an id match wins over a legacy name match", () => {
    const tricky: AudioDevice[] = [
      { deviceId: "x", label: "X", isDefault: false, legacyName: "y" },
      { deviceId: "y", label: "Y", isDefault: false, legacyName: "z" },
    ];
    expect(preselectedDeviceId(tricky, "y")).toBe("y");
  });

  it("never matches browser devices (no legacy name) by label", () => {
    const browser: AudioDevice[] = [
      { deviceId: "default", label: "Default - Mic", isDefault: true },
      { deviceId: "m1", label: "Mic", isDefault: false },
    ];
    expect(preselectedDeviceId(browser, "Mic")).toBe("default");
  });
});

describe("labelNamesSystemDefault", () => {
  it("is true only for the desktop app's own system default entry", () => {
    expect(labelNamesSystemDefault(devices[0])).toBe(true);
    expect(labelNamesSystemDefault(devices[1])).toBe(false);
    // A real device flagged as default (Windows/macOS) or a browser entry.
    expect(labelNamesSystemDefault({ deviceId: "wasapi:1", label: "Speakers", isDefault: true, curated: true })).toBe(false);
    expect(labelNamesSystemDefault({ deviceId: "default", label: "System default", isDefault: true })).toBe(false);
  });
});
