import {
  createVirtualizer as createTanStackVirtualizer,
  type VirtualItem,
} from "@tanstack/solid-virtual";

export type { VirtualItem };

interface VirtualizerOptions {
  get count(): number;
  getScrollElement: () => HTMLElement | null;
  estimateSize: (index: number) => number;
  overscan?: number;
}

/**
 * Thin wrapper around TanStack Virtual's createVirtualizer for Solid.js.
 *
 * Returns the TanStack virtualizer directly to preserve Solid's reactive
 * tracking. Previous versions wrapped methods in arrow functions which
 * could break reactivity in edge cases.
 */
export function createVirtualizer(options: VirtualizerOptions) {
  return createTanStackVirtualizer({
    get count() {
      return options.count;
    },
    getScrollElement: options.getScrollElement,
    estimateSize: options.estimateSize,
    overscan: options.overscan ?? 0,
  });
}
