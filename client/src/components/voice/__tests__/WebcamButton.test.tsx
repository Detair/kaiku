import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

const { voiceState, setVoiceState, startWebcam, stopWebcam } = await vi.hoisted(async () => {
  const { createStore } = await import("solid-js/store");
  const [voiceState, setVoiceState] = createStore({ state: "connected", webcamActive: false });
  return { voiceState, setVoiceState, startWebcam: vi.fn(), stopWebcam: vi.fn() };
});

vi.mock("@/stores/voice", () => ({
  voiceState,
  startWebcam: (...a: unknown[]) => startWebcam(...a),
  stopWebcam: (...a: unknown[]) => stopWebcam(...a),
}));

import WebcamButton from "../WebcamButton";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
  setVoiceState({ state: "connected", webcamActive: false });
});

describe("WebcamButton", () => {
  it("a failed start is announced to screen readers", async () => {
    startWebcam.mockResolvedValue({ ok: false, error: "The camera is in use." });
    render(() => <WebcamButton />);
    fireEvent.click(screen.getByRole("button"));
    expect((await screen.findByRole("alert")).textContent).toBe("The camera is in use.");
  });

  it("its hide timer does not outlive the button", async () => {
    vi.useFakeTimers();
    startWebcam.mockResolvedValue({ ok: false, error: "x" });
    const { unmount } = render(() => <WebcamButton />);
    fireEvent.click(screen.getByRole("button"));
    await vi.waitFor(() => expect(screen.queryByRole("alert")).not.toBeNull());
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("WebcamButton while the call reconnects", () => {
  it("the camera can be switched off while the call reconnects", () => {
    stopWebcam.mockResolvedValue(undefined);
    setVoiceState({ state: "reconnecting", webcamActive: true });
    render(() => <WebcamButton />);
    const button = screen.getByRole("button") as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    fireEvent.click(button);
    expect(stopWebcam).toHaveBeenCalledTimes(1);
  });

  it("the camera cannot be switched on while the call reconnects", () => {
    setVoiceState({ state: "reconnecting", webcamActive: false });
    render(() => <WebcamButton />);
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });
});
