import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, waitFor, fireEvent } from "@solidjs/testing-library";
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
const updateAudioSetting = vi.hoisted(() => vi.fn());
vi.mock("@/stores/settings", () => {
  const [settings] = createSignal({ audio });
  return {
    appSettings: settings,
    isSettingsLoading: () => false,
    updateAudioSetting,
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
  updateAudioSetting.mockClear();
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

  it("preselects a device saved by its legacy name without rewriting the setting", async () => {
    audio.input_device = "Webcam, USB Audio";
    adapter.getAudioDevices.mockResolvedValue({
      ok: true,
      value: {
        inputs: [
          { ...nativeList.inputs[0], legacyName: "Default ALSA Output" },
          { ...nativeList.inputs[1], legacyName: "USB Audio, USB Audio" },
          { ...nativeList.inputs[2], legacyName: "Webcam, USB Audio" },
        ],
        outputs: nativeList.outputs,
      },
    });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));

    expect(selects()[0].value).toBe("alsa:sysdefault:CARD=USB2");
    expect(updateAudioSetting).not.toHaveBeenCalled();
    expect(audio.input_device).toBe("Webcam, USB Audio");
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

  it("Windows/macOS: a physical default device keeps the follow-default option", async () => {
    const speakers = "wasapi:{0.0.0.00000000}.{speakers}";
    const headset = "wasapi:{0.0.0.00000000}.{headset}";
    adapter.getAudioDevices.mockResolvedValue({
      ok: true,
      value: {
        inputs: [{ deviceId: "wasapi:{0.0.1.00000000}.{mic}", label: "Microphone", isDefault: true, curated: true }],
        outputs: [
          { deviceId: speakers, label: "Speakers", isDefault: true, curated: true },
          { deviceId: headset, label: "Headset", isDefault: false, curated: true },
        ],
      },
    });
    adapter.setOutputDevice.mockResolvedValue({ ok: true });
    render(() => <AudioSettings />);
    await waitFor(() => expect(screen.getByText("Speakers")).toBeTruthy());

    const output = selects()[1];
    expect(optionTexts(output)).toEqual(["Default Device", "Speakers", "Headset"]);
    // Nothing saved: follow the system default, not the flagged hardware.
    expect(output.value).toBe("default");

    fireEvent.change(output, { target: { value: speakers } });
    await waitFor(() => expect(updateAudioSetting).toHaveBeenLastCalledWith("output_device", speakers));

    fireEvent.change(output, { target: { value: "default" } });
    await waitFor(() => expect(updateAudioSetting).toHaveBeenLastCalledWith("output_device", null));
  });
});
