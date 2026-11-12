/**
 * Positioned Emoji Picker
 *
 * Wrapper around EmojiPicker that uses floating-ui for smart positioning.
 * Handles viewport boundaries, automatic flipping, and click-outside detection.
 */

import { Component, onMount, onCleanup, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import {
  computePosition,
  autoUpdate,
  flip,
  shift,
  offset,
  size,
} from "@floating-ui/dom";
import EmojiPicker from "./EmojiPicker";
import { t } from "@/i18n";

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
  const [positioned, setPositioned] = createSignal(false);
  const [maxHeight, setMaxHeight] = createSignal<number | undefined>(undefined);

  const updatePosition = async () => {
    if (!pickerRef || !props.anchorEl) return;

    const { x, y } = await computePosition(props.anchorEl, pickerRef, {
      strategy: "fixed",
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
    setPositioned(true);
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
  const handleScroll = (e: Event) => {
    // Scrolling inside the picker itself (e.g. the emoji grid) must not close it
    if (pickerRef && e.target instanceof Node && pickerRef.contains(e.target)) {
      return;
    }
    props.onClose();
  };

  // Escape closes the picker only (A11Y-1): handled in the capture phase and
  // stopped, so a dialog underneath or a page shortcut never sees it.
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
  };

  // Escape works right away, before the click-outside listener is armed.
  document.addEventListener("keydown", handleKeyDown, true);
  onCleanup(() => {
    document.removeEventListener("keydown", handleKeyDown, true);
    // Focus goes back to the button that opened the picker.
    const active = document.activeElement;
    if (!active || active === document.body || pickerRef?.contains(active)) {
      if (props.anchorEl?.isConnected) props.anchorEl.focus();
    }
  });

  onMount(() => {
    // Keep the picker anchored when it or the anchor resizes, or the window resizes
    if (pickerRef && props.anchorEl) {
      const stopAutoUpdate = autoUpdate(props.anchorEl, pickerRef, () => {
        updatePosition().catch((err) => {
          console.error(
            "[PositionedEmojiPicker] Failed to calculate position:",
            err,
          );
          // Never leave the picker hidden if positioning fails
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
      window.addEventListener("scroll", handleScroll, true); // capture phase for all scrollable elements
    }, 0);

    onCleanup(() => {
      clearTimeout(timeoutId);
      document.removeEventListener("mousedown", handleClickOutside);
      window.removeEventListener("scroll", handleScroll, true);
    });
  });

  return (
    <Portal>
      <div
        ref={pickerRef}
        role="dialog"
        aria-label={t("chat.emoji.pickerLabel")}
        style={{
          position: "fixed",
          left: `${position().x}px`,
          top: `${position().y}px`,
          "z-index": "9999",
          visibility: positioned() ? "visible" : "hidden",
          ...(maxHeight() ? { "max-height": `${maxHeight()}px` } : {}),
        }}
      >
        <EmojiPicker
          onSelect={props.onSelect}
          onClose={props.onClose}
          guildId={props.guildId}
          maxHeight={maxHeight()}
        />
      </div>
    </Portal>
  );
};

export default PositionedEmojiPicker;
