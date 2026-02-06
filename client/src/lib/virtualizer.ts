interface VirtualItem {
  index: number;
  start: number;
  size: number;
}

interface ScrollToIndexOptions {
  align?: "start" | "center" | "end" | "auto";
  behavior?: ScrollBehavior;
}

interface VirtualizerOptions {
  get count(): number;
  getScrollElement: () => HTMLElement | null;
  estimateSize: (index: number) => number;
  overscan?: number;
}

function measure(options: VirtualizerOptions): { starts: number[]; sizes: number[]; total: number } {
  const count = options.count;
  const starts = new Array<number>(count);
  const sizes = new Array<number>(count);

  let total = 0;
  for (let i = 0; i < count; i += 1) {
    starts[i] = total;
    const size = Math.max(1, Math.floor(options.estimateSize(i)));
    sizes[i] = size;
    total += size;
  }

  return { starts, sizes, total };
}

export function createVirtualizer(options: VirtualizerOptions) {
  const getVirtualItems = (): VirtualItem[] => {
    const count = options.count;
    if (count <= 0) return [];

    const { starts, sizes } = measure(options);
    const scrollElement = options.getScrollElement();

    if (!scrollElement) {
      return starts.map((start, index) => ({ index, start, size: sizes[index] }));
    }

    const viewportStart = scrollElement.scrollTop;
    const viewportEnd = viewportStart + scrollElement.clientHeight;

    let startIndex = 0;
    while (startIndex < count && starts[startIndex] + sizes[startIndex] < viewportStart) {
      startIndex += 1;
    }

    let endIndex = startIndex;
    while (endIndex < count && starts[endIndex] <= viewportEnd) {
      endIndex += 1;
    }

    const overscan = Math.max(0, options.overscan ?? 0);
    const from = Math.max(0, startIndex - overscan);
    const to = Math.min(count, endIndex + overscan);

    const items: VirtualItem[] = [];
    for (let index = from; index < to; index += 1) {
      items.push({ index, start: starts[index], size: sizes[index] });
    }
    return items;
  };

  return {
    getVirtualItems,
    getTotalSize: () => measure(options).total,
    getScrollElement: options.getScrollElement,
    scrollToIndex: (index: number, scrollOptions: ScrollToIndexOptions = {}) => {
      const scrollElement = options.getScrollElement();
      if (!scrollElement || options.count <= 0) return;

      const target = Math.max(0, Math.min(index, options.count - 1));
      const { starts, sizes } = measure(options);
      const start = starts[target];
      const size = sizes[target];
      const viewport = scrollElement.clientHeight;

      let top = start;
      const align = scrollOptions.align ?? "auto";
      if (align === "end") {
        top = start - viewport + size;
      } else if (align === "center") {
        top = start - (viewport - size) / 2;
      } else if (align === "auto") {
        const currentTop = scrollElement.scrollTop;
        const currentBottom = currentTop + viewport;
        const itemEnd = start + size;

        if (start >= currentTop && itemEnd <= currentBottom) {
          return;
        }
        top = start < currentTop ? start : itemEnd - viewport;
      }

      scrollElement.scrollTo({
        top: Math.max(0, Math.floor(top)),
        behavior: scrollOptions.behavior,
      });
    },
    measureElement: (_el: Element) => {},
  };
}
