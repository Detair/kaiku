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

import MicTestPanel from "../MicTestPanel";

afterEach(cleanup);

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
});
