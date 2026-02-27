/**
 * Positioned Emoji Picker
 *
 * Wrapper around EmojiPicker that uses floating-ui for smart positioning.
 * Handles viewport boundaries, automatic flipping, and click-outside detection.
 */

import { Component, onMount, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { computePosition, flip, shift, offset, size } from "@floating-ui/dom";
import EmojiPicker from "./EmojiPicker";

interface PositionedEmojiPickerProps {
  /** Reference element to position relative to */
  anchorEl: HTMLElement;
  /** Callback when emoji is selected */
  onSelect: (emoji: string) => void;
  /** Callback when picker should close */
  onClose: () => void;
  /** Optional guild ID for custom emojis */
  guildId?: string;
}

const PositionedEmojiPicker: Component<PositionedEmojiPickerProps> = (
  props,
) => {
  let pickerRef: HTMLDivElement | undefined;
  const [position, setPosition] = createSignal({ x: 0, y: 0 });
  const [maxHeight, setMaxHeight] = createSignal<number | undefined>(undefined);

  const updatePosition = async () => {
    if (!pickerRef || !props.anchorEl) return;

    const { x, y } = await computePosition(props.anchorEl, pickerRef, {
      placement: "bottom-start",
      middleware: [
        offset(4), // Smaller gap for closer positioning
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
            // Limit picker height to available viewport space
            setMaxHeight(Math.min(384, availableHeight)); // max 384px (96 * 4)
          },
        }),
      ],
    });

    setPosition({ x: Math.round(x), y: Math.round(y) });
  };

  // Click outside detection
  const handleClickOutside = (e: MouseEvent) => {
    if (
      pickerRef &&
      !pickerRef.contains(e.target as Node) &&
      !props.anchorEl.contains(e.target as Node)
    ) {
      props.onClose();
    }
  };

  // Close on scroll
  const handleScroll = () => {
    props.onClose();
  };

  // Close on Escape key
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  onMount(() => {
    // Calculate initial position
    updatePosition().catch((err) => {
      console.error(
        "[PositionedEmojiPicker] Failed to calculate position:",
        err,
      );
    });

    // Add event listeners after a small delay to avoid immediate close
    const timeoutId = setTimeout(() => {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("keydown", handleKeyDown);
      window.addEventListener("scroll", handleScroll, true); // capture phase for all scrollable elements
    }, 0);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleScroll, true);
    };
  });

  const pos = position();
  const height = maxHeight();

  return (
    <Portal>
      <div
        ref={pickerRef}
        style={{
          position: "fixed",
          left: `${pos.x}px`,
          top: `${pos.y}px`,
          "z-index": "9999",
          ...(height ? { "max-height": `${height}px` } : {}),
        }}
      >
        <EmojiPicker
          onSelect={props.onSelect}
          onClose={props.onClose}
          guildId={props.guildId}
          maxHeight={height}
        />
      </div>
    </Portal>
  );
};

export default PositionedEmojiPicker;
