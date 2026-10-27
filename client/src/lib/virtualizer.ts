import {
  Virtualizer,
  elementScroll,
  observeElementOffset,
  observeElementRect,
  type VirtualItem,
} from "@tanstack/virtual-core";
import { createComputed, createSignal, onCleanup, onMount, untrack } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

export type { VirtualItem };

interface VirtualizerOptions {
  get count(): number;
  getScrollElement: () => HTMLElement | null;
  estimateSize: (index: number) => number;
  /**
   * Stable identity of the item at `index` (defaults to the index). Measured
   * sizes are cached per key, and each key keeps its virtual item object, so
   * a `<For>` row stays bound to the same item when items are inserted above
   * it. Read reactively: when the keys change without the count changing,
   * return a new function so the virtualizer recomputes.
   */
  readonly getItemKey?: (index: number) => string | number;
  overscan?: number;
}

/**
 * Solid adapter for TanStack Virtual's core.
 *
 * Replaces `@tanstack/solid-virtual`, which (a) called `measure()` on every
 * option change, dropping every measured size whenever the item count
 * changed, and (b) reconciled the items by index, so a row element was
 * reused for whichever item scrolled into its slot and its measurement was
 * credited to the wrong item. Both left rows overlapping or with gaps until
 * they happened to resize.
 *
 * `getVirtualItems()` is a store reconciled by key: iterate it with `<For>`
 * and read `item.index` / `item.start` reactively.
 */
export function createVirtualizer(options: VirtualizerOptions) {
  const [items, setItems] = createStore<VirtualItem[]>([]);
  const [totalSize, setTotalSize] = createSignal(0);

  const sync = (instance: Virtualizer<HTMLElement, Element>) => {
    untrack(() => {
      setItems(reconcile(instance.getVirtualItems(), { key: "key" }));
      setTotalSize(instance.getTotalSize());
    });
  };

  const resolve = () => ({
    count: options.count,
    getItemKey: options.getItemKey ?? ((index: number) => index),
    getScrollElement: options.getScrollElement,
    estimateSize: options.estimateSize,
    overscan: options.overscan ?? 0,
    observeElementRect,
    observeElementOffset,
    scrollToFn: elementScroll,
    onChange: (instance: Virtualizer<HTMLElement, Element>) => {
      instance._willUpdate();
      sync(instance);
    },
  });

  const instance = new Virtualizer<HTMLElement, Element>(untrack(resolve));

  // Count or key changes: recompute positions, keeping the measured sizes.
  createComputed(() => {
    instance.setOptions(resolve());
    instance._willUpdate();
    sync(instance);
  });

  onMount(() => {
    const cleanup = instance._didMount();
    instance._willUpdate();
    onCleanup(cleanup);
  });

  // The accessors are read inside the caller's tracked scopes (JSX, memos).
  return new Proxy(instance, {
    get(target, prop, receiver) {
      if (prop === "getVirtualItems") return () => items;
      // eslint-disable-next-line solid/reactivity
      if (prop === "getTotalSize") return () => totalSize();
      return Reflect.get(target, prop, receiver);
    },
  });
}
