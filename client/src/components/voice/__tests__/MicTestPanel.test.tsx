import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const adapter = vi.hoisted(() => ({
  getAudioDevices: vi.fn(),
  startMicTest: vi.fn(),
  stopMicTest: vi.fn().mockResolvedValue({ ok: true }),
  getMicTestLevel: vi.fn(() => 0),
}));
vi.mock("@/lib/webrtc", () => ({ createVoiceAdapter: async () => adapter }));
vi.mock("@/lib/browserCapabilities", () => ({ canSelectOutputDevice: () => true }));
const audio = vi.hoisted(() => ({
  input_device: null as string | null,
  output_device: null as string | null,
}));
vi.mock("@/stores/settings", () => ({ appSettings: () => ({ audio }) }));

import MicTestPanel from "../MicTestPanel";

afterEach(() => {
  cleanup();
  audio.input_device = null;
  audio.output_device = null;
});

describe("MicTestPanel", () => {
  it("re-enumerates devices once the mic test has permission", async () => {
    adapter.getAudioDevices
      .mockResolvedValueOnce({
        ok: true,
        value: { inputs: [], outputs: [], labelsAvailable: false },
      })
      .mockResolvedValue({
        ok: true,
        value: {
          inputs: [{ deviceId: "m1", label: "USB Mic", isDefault: false }],
          outputs: [],
          labelsAvailable: true,
        },
      });
    adapter.startMicTest.mockResolvedValue({ ok: true });

    render(() => <MicTestPanel />);
    await waitFor(() => expect(adapter.getAudioDevices).toHaveBeenCalledTimes(1));
    expect(screen.queryByText("USB Mic")).toBeNull();

    fireEvent.click(screen.getByText("Start Test"));
    await waitFor(() => expect(screen.getByText("USB Mic")).toBeTruthy());
  });

  it("preselects the saved devices by id, else the system default entry", async () => {
    audio.input_device = "m2";
    audio.output_device = "gone";
    adapter.getAudioDevices.mockReset().mockResolvedValue({
      ok: true,
      value: {
        inputs: [
          { deviceId: "default", label: "Default - USB Mic", isDefault: true },
          { deviceId: "m1", label: "USB Mic", isDefault: false },
          { deviceId: "m2", label: "USB Mic", isDefault: false },
        ],
        outputs: [
          { deviceId: "s1", label: "Speakers", isDefault: false },
          { deviceId: "default", label: "Default - Speakers", isDefault: true },
        ],
        labelsAvailable: true,
      },
    });

    render(() => <MicTestPanel />);
    await waitFor(() => expect(screen.getAllByText("USB Mic").length).toBe(2));
    const [input, output] = screen.getAllByRole("combobox") as HTMLSelectElement[];
    expect(input.value).toBe("m2");
    expect(input.selectedIndex).toBe(2);
    expect(output.value).toBe("default");
  });
});
