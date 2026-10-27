import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, cleanup, screen } from "@solidjs/testing-library";

const autoUpdateCleanup = vi.fn();
const autoUpdateMock = vi.fn((...args: unknown[]) => {
  (args[2] as () => void)();
  return autoUpdateCleanup;
});
const computePositionMock = vi.fn(async (..._args: unknown[]) => ({
  x: 33,
  y: 44,
}));

vi.mock("@floating-ui/dom", () => ({
  computePosition: (...args: unknown[]) => computePositionMock(...args),
  autoUpdate: (...args: unknown[]) => autoUpdateMock(...args),
  flip: vi.fn(() => ({})),
  shift: vi.fn(() => ({})),
  offset: vi.fn(() => ({})),
  size: vi.fn(() => ({})),
}));

import PopupList from "../PopupList";

const flush = () => new Promise((r) => setTimeout(r, 5));
const items = [
  { id: "a", label: "Alice" },
  { id: "b", label: "Bob" },
];

function mountList(
  onClose: () => void,
  anchor: HTMLElement,
  list: typeof items = items,
) {
  return render(() => (
    <PopupList
      anchorEl={anchor}
      items={list}
      selectedIndex={0}
      onSelect={() => {}}
      onClose={onClose}
      onSelectionChange={() => {}}
    />
  ));
}

describe("PopupList", () => {
  let anchor: HTMLElement;

  beforeEach(() => {
    cleanup();
    autoUpdateCleanup.mockClear();
    autoUpdateMock.mockClear();
    computePositionMock.mockReset();
    computePositionMock.mockResolvedValue({ x: 33, y: 44 });
    anchor?.remove();
    anchor = document.createElement("textarea");
    document.body.appendChild(anchor);
  });

  it("applies the computed position and becomes visible", async () => {
    const view = mountList(vi.fn(), anchor);
    await flush();
    const box = screen.getByRole("listbox");
    expect(box.style.left).toBe("33px");
    expect(box.style.top).toBe("44px");
    expect(box.style.visibility).toBe("visible");
    view.unmount();
  });

  it("becomes visible if computePosition rejects", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    computePositionMock.mockRejectedValue(new Error("boom"));
    const view = mountList(vi.fn(), anchor);
    await flush();
    expect(screen.getByRole("listbox").style.visibility).toBe("visible");
    err.mockRestore();
    view.unmount();
  });

  it("removes listeners and stops autoUpdate on unmount", async () => {
    const addDoc = vi.spyOn(document, "addEventListener");
    const removeDoc = vi.spyOn(document, "removeEventListener");
    const view = mountList(vi.fn(), anchor);
    await flush();
    const added = addDoc.mock.calls.filter((c) => c[0] === "keydown");
    expect(added).toHaveLength(1);
    view.unmount();
    const removed = removeDoc.mock.calls.filter((c) => c[0] === "keydown");
    expect(removed[0][1]).toBe(added[0][1]);
    expect(autoUpdateCleanup).toHaveBeenCalledTimes(1);
    addDoc.mockRestore();
    removeDoc.mockRestore();
  });

  it("keydown after unmount does not throw or call onClose", async () => {
    const onClose = vi.fn();
    const view = mountList(onClose, anchor);
    await flush();
    view.unmount();
    expect(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    }).not.toThrow();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("ignores scroll inside the list but closes on outside scroll", async () => {
    const onClose = vi.fn();
    const view = mountList(onClose, anchor);
    await flush();
    screen.getByRole("listbox").dispatchEvent(new Event("scroll"));
    expect(onClose).not.toHaveBeenCalled();
    const outside = document.createElement("div");
    document.body.appendChild(outside);
    outside.dispatchEvent(new Event("scroll"));
    expect(onClose).toHaveBeenCalledTimes(1);
    view.unmount();
  });

  it("ignores keys when there are no items", async () => {
    const onClose = vi.fn();
    const view = mountList(onClose, anchor, []);
    await flush();
    const ev = new KeyboardEvent("keydown", {
      key: "Enter",
      cancelable: true,
    });
    document.dispatchEvent(ev);
    const down = new KeyboardEvent("keydown", {
      key: "ArrowDown",
      cancelable: true,
    });
    document.dispatchEvent(down);
    expect(ev.defaultPrevented).toBe(false);
    expect(down.defaultPrevented).toBe(false);
    view.unmount();
  });

  it("ignores keys during IME composition", async () => {
    const view = mountList(vi.fn(), anchor);
    await flush();
    const ev = new KeyboardEvent("keydown", {
      key: "Enter",
      cancelable: true,
      isComposing: true,
    });
    document.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
    view.unmount();
  });
});
