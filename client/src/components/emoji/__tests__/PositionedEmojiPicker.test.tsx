import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, cleanup, screen } from "@solidjs/testing-library";

const autoUpdateCleanup = vi.fn();
const autoUpdateMock = vi.fn((..._args: unknown[]) => autoUpdateCleanup);

vi.mock("@floating-ui/dom", () => ({
  computePosition: vi.fn(async () => ({ x: 10, y: 20 })),
  autoUpdate: (...args: unknown[]) => autoUpdateMock(...args),
  flip: vi.fn(() => ({})),
  shift: vi.fn(() => ({})),
  offset: vi.fn(() => ({})),
  size: vi.fn(() => ({})),
}));

vi.mock("../EmojiPicker", () => ({
  default: () => (
    <div data-testid="picker">
      <div data-testid="grid">grid</div>
    </div>
  ),
}));

import PositionedEmojiPicker from "../PositionedEmojiPicker";

const flush = () => new Promise((r) => setTimeout(r, 5));

function mountPicker(onClose: () => void, anchor: HTMLElement) {
  return render(() => (
    <PositionedEmojiPicker
      anchorEl={anchor}
      onSelect={() => {}}
      onClose={onClose}
    />
  ));
}

describe("PositionedEmojiPicker lifecycle", () => {
  let anchor: HTMLButtonElement;

  beforeEach(() => {
    cleanup();
    autoUpdateCleanup.mockClear();
    autoUpdateMock.mockClear();
    anchor?.remove();
    anchor = document.createElement("button");
    document.body.appendChild(anchor);
  });

  it("removes document/window listeners on unmount", async () => {
    const addDoc = vi.spyOn(document, "addEventListener");
    const removeDoc = vi.spyOn(document, "removeEventListener");
    const view = mountPicker(vi.fn(), anchor);
    await flush();
    const added = addDoc.mock.calls.filter((c) => c[0] === "mousedown");
    expect(added).toHaveLength(1);
    view.unmount();
    const removed = removeDoc.mock.calls.filter((c) => c[0] === "mousedown");
    expect(removed).toHaveLength(1);
    expect(removed[0][1]).toBe(added[0][1]);
    addDoc.mockRestore();
    removeDoc.mockRestore();
  });

  it("a previous instance does not close a reopened picker", async () => {
    const onClose = vi.fn();
    const first = mountPicker(onClose, anchor);
    await flush();
    first.unmount();
    const second = mountPicker(onClose, anchor);
    await flush();
    const inside = screen.getByTestId("picker");
    inside.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(onClose).not.toHaveBeenCalled();
    second.unmount();
  });

  it("closes on mousedown outside", async () => {
    const onClose = vi.fn();
    const view = mountPicker(onClose, anchor);
    await flush();
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(onClose).toHaveBeenCalledTimes(1);
    view.unmount();
  });

  it("ignores scroll inside the picker but closes on outside scroll", async () => {
    const onClose = vi.fn();
    const view = mountPicker(onClose, anchor);
    await flush();
    screen.getByTestId("grid").dispatchEvent(new Event("scroll"));
    expect(onClose).not.toHaveBeenCalled();

    const outside = document.createElement("div");
    document.body.appendChild(outside);
    outside.dispatchEvent(new Event("scroll"));
    expect(onClose).toHaveBeenCalledTimes(1);
    view.unmount();
  });

  it("starts autoUpdate and stops it on unmount", async () => {
    const view = mountPicker(vi.fn(), anchor);
    await flush();
    expect(autoUpdateMock).toHaveBeenCalledTimes(1);
    expect(autoUpdateMock.mock.calls[0][0]).toBe(anchor);
    expect(autoUpdateCleanup).not.toHaveBeenCalled();
    view.unmount();
    expect(autoUpdateCleanup).toHaveBeenCalledTimes(1);
  });
});
