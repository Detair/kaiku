import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

const { voiceState, startWebcam, stopWebcam } = await vi.hoisted(async () => {
  const { createStore } = await import("solid-js/store");
  const [voiceState] = createStore({ state: "connected", webcamActive: false });
  return { voiceState, startWebcam: vi.fn(), stopWebcam: vi.fn() };
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
