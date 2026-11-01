import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, waitFor, fireEvent } from "@solidjs/testing-library";

const adapter = vi.hoisted(() => ({
  getAudioDevices: vi.fn(),
  setInputDevice: vi.fn(),
  setOutputDevice: vi.fn(),
  isNoiseSuppressionEnabled: vi.fn(() => true),
}));
const audio = vi.hoisted(() => ({
  input_device: null as string | null,
  output_device: null as string | null,
}));

vi.mock("@/lib/webrtc", () => ({ createVoiceAdapter: async () => adapter }));
vi.mock("@/lib/browserCapabilities", () => ({ canSelectOutputDevice: () => true }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => true }));
vi.mock("@/stores/voice", () => ({ setSpeaking: vi.fn() }));
const updateAudioSetting = vi.hoisted(() => vi.fn());
vi.mock("@/stores/settings", () => ({
  appSettings: () => ({ audio }),
  updateAudioSetting,
}));

import AudioDeviceSettings from "../AudioDeviceSettings";

const list = {
  inputs: [
    { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
    { deviceId: "alsa:sysdefault:CARD=A", label: "USB Mic", isDefault: false, curated: true },
    { deviceId: "alsa:sysdefault:CARD=B", label: "USB Mic", isDefault: false, curated: true },
  ],
  outputs: [
    { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
    { deviceId: "alsa:hdmi:CARD=HDMI,DEV=0", label: "HDMI", isDefault: false, curated: true },
  ],
};

afterEach(() => {
  cleanup();
  audio.input_device = null;
  audio.output_device = null;
});

const pickers = async () => {
  await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));
  return screen.getAllByRole("combobox") as HTMLSelectElement[];
};

describe("AudioDeviceSettings device pickers", () => {
  it("preselects the saved devices by id", async () => {
    audio.input_device = "alsa:sysdefault:CARD=B";
    audio.output_device = "alsa:hdmi:CARD=HDMI,DEV=0";
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: list });
    render(() => <AudioDeviceSettings onClose={() => {}} />);

    const [input, output] = await pickers();
    expect(input.value).toBe("alsa:sysdefault:CARD=B");
    expect(input.selectedIndex).toBe(2);
    expect(output.value).toBe("alsa:hdmi:CARD=HDMI,DEV=0");
  });

  it("shows the system default once and preselects it when nothing is saved", async () => {
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: list });
    render(() => <AudioDeviceSettings onClose={() => {}} />);

    const [input] = await pickers();
    expect(input.options.length).toBe(3); // no "Default Device" next to the alias
    expect([...input.options].filter((o) => o.value === "alsa:default").length).toBe(1);
    expect(input.value).toBe("alsa:default");
  });

  it("adds (Default) only where the label does not already say it", async () => {
    adapter.getAudioDevices.mockResolvedValue({ ok: true, value: list });
    render(() => <AudioDeviceSettings onClose={() => {}} />);
    const [input] = await pickers();
    expect(input.options[0].textContent).toBe("System default (PipeWire)");
    cleanup();

    adapter.getAudioDevices.mockResolvedValue({
      ok: true,
      value: {
        inputs: [
          { deviceId: "m0", label: "Headset", isDefault: true },
          { deviceId: "m1", label: "USB Mic", isDefault: false },
          { deviceId: "m2", label: "USB Mic", isDefault: false },
        ],
        outputs: [],
      },
    });
    render(() => <AudioDeviceSettings onClose={() => {}} />);
    const [browserInput] = await pickers();
    // No alias entry: the follow-default option comes first.
    expect(browserInput.options[0].textContent).toBe("Default Device");
    expect(browserInput.options[1].textContent).toBe("Headset (Default)");
  });

  it("Windows/macOS: offers follow-default next to a physical default device", async () => {
    const speakers = "wasapi:{0.0.0.00000000}.{speakers}";
    adapter.getAudioDevices.mockResolvedValue({
      ok: true,
      value: {
        inputs: [
          { deviceId: "wasapi:mic", label: "Microphone", isDefault: true, curated: true },
          { deviceId: "wasapi:a", label: "USB Mic", isDefault: false, curated: true },
          { deviceId: "wasapi:b", label: "USB Mic", isDefault: false, curated: true },
        ],
        outputs: [
          { deviceId: speakers, label: "Speakers", isDefault: true, curated: true },
          { deviceId: "wasapi:headset", label: "Headset", isDefault: false, curated: true },
        ],
      },
    });
    adapter.setOutputDevice.mockResolvedValue({ ok: true });
    updateAudioSetting.mockClear();
    render(() => <AudioDeviceSettings onClose={() => {}} />);

    const [, output] = await pickers();
    expect([...output.options].map((o) => o.textContent)).toEqual([
      "Default Device",
      "Speakers (Default)",
      "Headset",
    ]);
    expect(output.value).toBe("default");

    fireEvent.change(output, { target: { value: speakers } });
    await waitFor(() => expect(updateAudioSetting).toHaveBeenLastCalledWith("output_device", speakers));

    fireEvent.change(output, { target: { value: "default" } });
    await waitFor(() => expect(updateAudioSetting).toHaveBeenLastCalledWith("output_device", null));
    // The running session moves to the current default device.
    expect(adapter.setOutputDevice).toHaveBeenLastCalledWith(speakers);
  });
});
