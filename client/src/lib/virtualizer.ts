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

interface ScrollToIndexOptions {
  align?: "start" | "center" | "end" | "auto";
  behavior?: "auto" | "smooth";
}

export function createVirtualizer(options: VirtualizerOptions) {
  const virtualizer = createTanStackVirtualizer({
    get count() {
      return options.count;
    },
    getScrollElement: options.getScrollElement,
    estimateSize: options.estimateSize,
    overscan: options.overscan ?? 0,
  });

  return {
    getVirtualItems: (): VirtualItem[] => virtualizer.getVirtualItems(),
    getTotalSize: (): number => virtualizer.getTotalSize(),
    getScrollElement: options.getScrollElement,
    scrollToIndex: (
      index: number,
      scrollOptions: ScrollToIndexOptions = {},
    ) => {
      virtualizer.scrollToIndex(index, scrollOptions);
    },
    measureElement: (node: Element | null | undefined) => {
      if (node) {
        virtualizer.measureElement(node as HTMLElement);
      }
    },
  };
}
