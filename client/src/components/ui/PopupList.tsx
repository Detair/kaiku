/**
 * Popup List Component
 *
 * Generic keyboard-navigable list with smart positioning using floating-ui.
 * Used for autocomplete suggestions, command palette, etc.
 */

import {
  Component,
  For,
  JSX,
  createSignal,
  onMount,
  onCleanup,
  createEffect,
} from "solid-js";
import { Portal } from "solid-js/web";
import {
  computePosition,
  flip,
  shift,
  offset,
  size,
  autoUpdate,
} from "@floating-ui/dom";

export interface PopupListItem {
  id: string;
  label: string;
  /** Optional icon/avatar element */
  icon?: JSX.Element;
  /** Optional secondary text */
  description?: string;
}

interface PopupListProps {
  /** Reference element to position relative to */
  anchorEl: HTMLElement;
  /** List items to display */
  items: PopupListItem[];
  /** Currently selected index */
  selectedIndex: number;
  /** Callback when an item is selected */
  onSelect: (item: PopupListItem, index: number) => void;
  /** Callback when popup should close */
  onClose: () => void;
  /** Callback when selection changes (for keyboard navigation) */
  onSelectionChange: (index: number) => void;
}

const PopupList: Component<PopupListProps> = (props) => {
  let listRef: HTMLDivElement | undefined;
  const [position, setPosition] = createSignal({ x: 0, y: 0 });
  const [positioned, setPositioned] = createSignal(false);
  const [maxHeight, setMaxHeight] = createSignal<number | undefined>(undefined);

  const updatePosition = async () => {
    if (!listRef || !props.anchorEl) return;

    const { x, y } = await computePosition(props.anchorEl, listRef, {
      strategy: "fixed",
      placement: "bottom-start",
      middleware: [
        offset(4),
        flip({
          fallbackPlacements: ["top-start", "bottom-end", "top-end"],
          padding: 8,
        }),
        shift({
          padding: 8,
        }),
        size({
          padding: 8,
          apply({ availableHeight }) {
            // Limit list height to available viewport space
            setMaxHeight(Math.min(320, availableHeight)); // max 320px (~8 items)
          },
        }),
      ],
    });

    setPosition({ x: Math.round(x), y: Math.round(y) });
    setPositioned(true);
  };

  // Scroll selected item into view whenever the selection changes
  createEffect(() => {
    const index = props.selectedIndex;
    const el = listRef?.querySelector<HTMLElement>(`#suggestion-${index}`);
    el?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  });

  // Click outside detection
  const handleClickOutside = (e: MouseEvent) => {
    if (
      listRef &&
      !listRef.contains(e.target as Node) &&
      !props.anchorEl.contains(e.target as Node)
    ) {
      props.onClose();
    }
  };

  // Close on scroll
  const handleScroll = (e: Event) => {
    // Scrolling inside the list itself must not close it
    if (listRef && e.target instanceof Node && listRef.contains(e.target)) {
      return;
    }
    props.onClose();
  };

  // Keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const itemCount = props.items.length;

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        props.onSelectionChange((props.selectedIndex + 1) % itemCount);
        break;
      case "ArrowUp":
        e.preventDefault();
        props.onSelectionChange(
          (props.selectedIndex - 1 + itemCount) % itemCount,
        );
        break;
      case "Enter":
      case "Tab":
        e.preventDefault();
        if (props.items[props.selectedIndex]) {
          props.onSelect(props.items[props.selectedIndex], props.selectedIndex);
        }
        break;
      case "Escape":
        e.preventDefault();
        props.onClose();
        break;
    }
  };

  onMount(() => {
    // autoUpdate runs updatePosition immediately and on anchor/list/window changes
    if (listRef && props.anchorEl) {
      const stopAutoUpdate = autoUpdate(props.anchorEl, listRef, () => {
        updatePosition().catch((err) => {
          console.error("[PopupList] Failed to calculate position:", err);
          // Never leave the list hidden if positioning fails
          setPositioned(true);
        });
      });
      onCleanup(stopAutoUpdate);
    } else {
      setPositioned(true);
    }

    // Add event listeners after a small delay to avoid immediate close
    const timeoutId = setTimeout(() => {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("keydown", handleKeyDown);
      window.addEventListener("scroll", handleScroll, true);
    }, 0);

    onCleanup(() => {
      clearTimeout(timeoutId);
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleScroll, true);
    });
  });

  return (
    <Portal>
      <div
        ref={listRef}
        role="listbox"
        aria-label="Suggestions"
        aria-activedescendant={
          props.items[props.selectedIndex]
            ? `suggestion-${props.selectedIndex}`
            : undefined
        }
        style={{
          position: "fixed",
          left: `${position().x}px`,
          top: `${position().y}px`,
          "z-index": "9999",
          visibility: positioned() ? "visible" : "hidden",
          ...(maxHeight() ? { "max-height": `${maxHeight()}px` } : {}),
          "background-color": "var(--color-surface-layer2, #2A2A3C)", // Fallback to focused-hybrid color
        }}
        class="bg-surface-layer2 border border-white/10 rounded-lg shadow-xl overflow-y-auto"
      >
        <div class="py-1" role="none">
          <For each={props.items}>
            {(item, index) => (
              <div
                role="option"
                id={`suggestion-${index()}`}
                aria-selected={index() === props.selectedIndex}
                class="px-3 py-2 cursor-pointer transition-colors"
                classList={{
                  "bg-accent-primary/20": index() === props.selectedIndex,
                  "hover:bg-white/5": index() !== props.selectedIndex,
                }}
                onClick={() => props.onSelect(item, index())}
                onMouseEnter={() => props.onSelectionChange(index())}
              >
                <div class="flex items-center gap-2">
                  {item.icon && <div class="flex-shrink-0">{item.icon}</div>}
                  <div class="flex-1 min-w-0">
                    <div class="text-sm text-text-primary font-medium truncate">
                      {item.label}
                    </div>
                    {item.description && (
                      <div class="text-xs text-text-secondary truncate">
                        {item.description}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}
          </For>
        </div>
      </div>
    </Portal>
  );
};

export default PopupList;
