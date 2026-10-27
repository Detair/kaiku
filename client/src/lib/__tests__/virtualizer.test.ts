import { describe, it, expect, beforeAll } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { createVirtualizer } from "../virtualizer";

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

function setup(initialKeys: string[]) {
  const scroller = document.createElement("div");
  Object.defineProperty(scroller, "clientHeight", { value: 10_000 });
  Object.defineProperty(scroller, "offsetHeight", { value: 10_000 });
  return createRoot((dispose) => {
    const [keys, setKeys] = createSignal(initialKeys);
    const virtualizer = createVirtualizer({
      get count() {
        return keys().length;
      },
      get getItemKey() {
        const k = keys();
        return (index: number) => k[index];
      },
      getScrollElement: () => scroller,
      estimateSize: () => 50,
      overscan: 100,
    });
    return { virtualizer, setKeys, dispose };
  });
}

describe("createVirtualizer", () => {
  it("keeps each key's item object and measured size when items are prepended", () => {
    const { virtualizer, setKeys, dispose } = setup(["a", "b", "c"]);
    const itemOf = (key: string) =>
      virtualizer.getVirtualItems().find((v) => v.key === key);

    virtualizer.resizeItem(1, 200); // "b" measured at 200px
    const b = itemOf("b");
    expect(b?.size).toBe(200);

    setKeys(["y", "z", "a", "b", "c"]);

    // same object (a <For> row stays bound to "b"), now at index 3
    expect(itemOf("b")).toBe(b);
    expect(b?.index).toBe(3);
    // the measured size followed the key and was not dropped by the count change
    expect(b?.size).toBe(200);
    expect(itemOf("c")?.start).toBe(50 * 3 + 200);
    expect(virtualizer.getTotalSize()).toBe(50 * 4 + 200);
    dispose();
  });
});
