import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, waitFor } from "@solidjs/testing-library";

const adapter = vi.hoisted(() => ({
  getAudioDevices: vi.fn(),
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
vi.mock("@/stores/settings", () => ({
  appSettings: () => ({ audio }),
  updateAudioSetting: vi.fn(),
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
    expect(input.options.length).toBe(3);
    expect([...input.options].filter((o) => o.value === "alsa:default").length).toBe(1);
    expect(input.value).toBe("alsa:default");
  });
});
