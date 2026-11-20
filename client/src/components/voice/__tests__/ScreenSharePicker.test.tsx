import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@solidjs/testing-library";

const { voiceState, setVoiceState, startScreenShare, stopScreenShare, cancelMediaStart } = await vi.hoisted(
  async () => {
    const { createStore } = await import("solid-js/store");
    const [voiceState, setVoiceState] = createStore({ state: "connected", screenSharing: false });
    return {
      voiceState,
      setVoiceState,
      startScreenShare: vi.fn(),
      stopScreenShare: vi.fn().mockResolvedValue(undefined),
      cancelMediaStart: vi.fn().mockResolvedValue(undefined),
    };
  },
);

vi.mock("@/stores/voice", () => ({
  voiceState,
  startScreenShare: (...a: unknown[]) => startScreenShare(...a),
  stopScreenShare: (...a: unknown[]) => stopScreenShare(...a),
  cancelMediaStart: (...a: unknown[]) => cancelMediaStart(...a),
}));

import ScreenShareQualityPicker from "../ScreenShareQualityPicker";
import ScreenShareButton from "../ScreenShareButton";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  setVoiceState({ screenSharing: false });
});

describe("ScreenShareQualityPicker", () => {
  it("offers three plain tiers, Normal preselected, no Premium", () => {
    render(() => <ScreenShareQualityPicker onClose={() => {}} />);
    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    expect(radios.map((r) => r.value)).toEqual(["low", "medium", "high"]);
    expect(radios[1].checked).toBe(true);
    expect(screen.getByText("Normal")).toBeTruthy();
    expect(screen.getByText("Sharp (for games)")).toBeTruthy();
    expect(screen.queryByText(/premium|fps/i)).toBeNull();
  });

  it("a cancelled browser picker closes quietly (no microphone text)", async () => {
    startScreenShare.mockResolvedValue({ ok: false });
    const onClose = vi.fn();
    render(() => <ScreenShareQualityPicker onClose={onClose} />);
    fireEvent.click(screen.getByTestId("screen-share-start"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(/microphone/i)).toBeNull();
  });

  it("a refusal stays visible", async () => {
    startScreenShare.mockResolvedValue({ ok: false, error: "Not allowed here." });
    const onClose = vi.fn();
    render(() => <ScreenShareQualityPicker onClose={onClose} />);
    fireEvent.click(screen.getByTestId("screen-share-start"));
    expect((await screen.findByRole("alert")).textContent).toBe("Not allowed here.");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a cancelled system dialog (Linux) keeps the picker with a calm note", async () => {
    startScreenShare.mockResolvedValue({ ok: false, note: "No screen was chosen." });
    const onClose = vi.fn();
    render(() => <ScreenShareQualityPicker systemPicker onClose={onClose} />);
    fireEvent.click(screen.getByTestId("screen-share-start"));
    expect((await screen.findByRole("status")).textContent).toBe("No screen was chosen.");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("Cancel while the start runs aborts the start", async () => {
    startScreenShare.mockReturnValue(new Promise(() => {}));
    const onClose = vi.fn();
    render(() => <ScreenShareQualityPicker onClose={onClose} />);
    fireEvent.click(screen.getByTestId("screen-share-start"));
    await waitFor(() => expect(screen.getByTestId("screen-share-start").textContent).toBe("Starting…"));
    fireEvent.click(screen.getByText("Cancel"));
    expect(cancelMediaStart).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("Cancel before starting does not touch any start", () => {
    const onClose = vi.fn();
    render(() => <ScreenShareQualityPicker onClose={onClose} />);
    fireEvent.click(screen.getByText("Cancel"));
    expect(cancelMediaStart).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("opens above a dialog that is already open", () => {
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.dialog);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => <ScreenShareQualityPicker onClose={() => {}} />);
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    let el: HTMLElement | null = screen.getByRole("dialog");
    let z = 0;
    for (; el; el = el.parentElement) {
      if (el.style.zIndex) {
        z = Number(el.style.zIndex);
        break;
      }
    }
    expect(z).toBeGreaterThan(earlier);
  });
});

describe("ScreenShareButton", () => {
  it("while sharing, the same button stops the share", async () => {
    vi.stubGlobal("navigator", { ...navigator, mediaDevices: { getDisplayMedia: vi.fn() } });
    const onShowQualityPicker = vi.fn();
    setVoiceState({ screenSharing: true });
    render(() => <ScreenShareButton onShowQualityPicker={onShowQualityPicker} />);
    const button = screen.getByTestId("screen-share-toggle");
    expect(button.getAttribute("aria-label")).toBe("Stop sharing");
    // The label says it; no "pressed" on top of "Stop sharing".
    expect(button.hasAttribute("aria-pressed")).toBe(false);
    fireEvent.click(button);
    await waitFor(() => expect(stopScreenShare).toHaveBeenCalled());
    expect(onShowQualityPicker).not.toHaveBeenCalled();
  });

  it("when not sharing it opens the picker", () => {
    vi.stubGlobal("navigator", { ...navigator, mediaDevices: { getDisplayMedia: vi.fn() } });
    const onShowQualityPicker = vi.fn();
    render(() => <ScreenShareButton onShowQualityPicker={onShowQualityPicker} />);
    fireEvent.click(screen.getByTestId("screen-share-toggle"));
    expect(onShowQualityPicker).toHaveBeenCalled();
  });
});
