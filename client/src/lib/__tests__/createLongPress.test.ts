import { describe, it, expect, vi, afterEach } from "vitest";
import { createLongPress } from "../createLongPress";

const pointer = (pointerType: string, button = 0) =>
  ({ pointerType, button, clientX: 10, clientY: 20 }) as PointerEvent;

const contextMenu = () => {
  const e = new Event("contextmenu", { cancelable: true });
  return e;
};

afterEach(() => vi.useRealTimers());

describe("createLongPress", () => {
  // Linux and macOS fire contextmenu on mousedown, while the press is pending.
  it("lets a right-click open the context menu", () => {
    const lp = createLongPress(vi.fn());
    lp.onPointerDown(pointer("mouse", 2));
    const e = contextMenu();
    lp.onContextMenu(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it("lets a pen barrel-button click open the context menu", () => {
    const lp = createLongPress(vi.fn());
    lp.onPointerDown(pointer("pen", 2));
    const e = contextMenu();
    lp.onContextMenu(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it("never fires a long press for a mouse", () => {
    vi.useFakeTimers();
    const onLongPress = vi.fn();
    const lp = createLongPress(onLongPress);
    lp.onPointerDown(pointer("mouse", 0));
    vi.advanceTimersByTime(600);
    expect(onLongPress).not.toHaveBeenCalled();
  });

  it("fires on a touch long press and suppresses the native menu", () => {
    vi.useFakeTimers();
    const onLongPress = vi.fn();
    const lp = createLongPress(onLongPress);
    lp.onPointerDown(pointer("touch"));
    vi.advanceTimersByTime(600);
    expect(onLongPress).toHaveBeenCalledWith(10, 20);
    const e = contextMenu();
    lp.onContextMenu(e);
    expect(e.defaultPrevented).toBe(true);
  });
});
