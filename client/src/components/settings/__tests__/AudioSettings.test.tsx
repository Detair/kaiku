import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

const adapter = vi.hoisted(() => ({
  getAudioDevices: vi.fn(),
  setInputDevice: vi.fn(),
  setOutputDevice: vi.fn(),
}));
const audio = vi.hoisted(() => ({
  input_device: null as string | null,
  output_device: null as string | null,
  input_volume: 100,
  output_volume: 100,
}));

vi.mock("@/lib/webrtc", () => ({
  createVoiceAdapter: async () => adapter,
  getVoiceAdapter: () => null,
}));
vi.mock("@/lib/browserCapabilities", () => ({ canSelectOutputDevice: () => true }));
vi.mock("@/stores/settings", () => {
  const [settings] = createSignal({ audio });
  return {
    appSettings: settings,
    isSettingsLoading: () => false,
    updateAudioSetting: vi.fn(),
  };
});

import AudioSettings from "../AudioSettings";

const nativeList = {
  inputs: [
    { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
    { deviceId: "alsa:sysdefault:CARD=USB", label: "USB Mic", isDefault: false, curated: true },
    { deviceId: "alsa:sysdefault:CARD=USB2", label: "USB Mic", isDefault: false, curated: true },
  ],
  outputs: [
    { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
    { deviceId: "alsa:hdmi:CARD=HDMI,DEV=0", label: "HDMI", isDefault: false, curated: true },
  ],
};

const selects = () => screen.getAllByRole("combobox") as HTMLSelectElement[];
const optionTexts = (el: HTMLSelectElement) => [...el.options].map((o) => o.textContent);

beforeEach(() => {
  audio.input_device = null;
  audio.output_device = null;
});
afterEach(cleanup);

describe("AudioSettings device pickers", () => {
  it("lists the backend's system default once, without a second synthetic Default", async () => {
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: nativeList });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));

    const [input, output] = selects();
    expect(optionTexts(input)).toEqual(["System default (PipeWire)", "USB Mic", "USB Mic"]);
    expect(optionTexts(output)).toEqual(["System default (PipeWire)", "HDMI"]);
    expect(screen.queryByText("Default Device")).toBeNull();
    // Nothing saved: the system default entry is preselected.
    expect(input.value).toBe("alsa:default");
    expect(output.value).toBe("alsa:default");
  });

  it("preselects the saved device by id, also when labels repeat", async () => {
    audio.input_device = "alsa:sysdefault:CARD=USB2";
    audio.output_device = "alsa:hdmi:CARD=HDMI,DEV=0";
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: nativeList });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));

    const [input, output] = selects();
    expect(input.value).toBe("alsa:sysdefault:CARD=USB2");
    expect(input.selectedIndex).toBe(2);
    expect(output.value).toBe("alsa:hdmi:CARD=HDMI,DEV=0");
  });

  it("falls back to the system default when the saved id is not listed", async () => {
    audio.input_device = "USB Mic"; // a label, not an id
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: nativeList });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));
    expect(selects()[0].value).toBe("alsa:default");
  });

  it("keeps the synthetic Default option when the list has no default entry", async () => {
    adapter.getAudioDevices.mockResolvedValue({
      ok: true,
      value: {
        inputs: [{ deviceId: "m1", label: "Headset", isDefault: false }],
        outputs: [{ deviceId: "s1", label: "Speakers", isDefault: false }],
        labelsAvailable: true,
      },
    });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getByText("Headset")).toBeTruthy());

    const [input] = selects();
    expect(optionTexts(input)).toEqual(["Default Device", "Headset"]);
    expect(input.value).toBe("default");
  });
});
