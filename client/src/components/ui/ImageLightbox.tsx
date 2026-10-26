/**
 * ImageLightbox — full-screen overlay for viewing images.
 *
 * Click outside or press Escape to close.
 */

import { Component, onMount, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { X } from "lucide-solid";
import { t } from "@/i18n";

interface ImageLightboxProps {
  src: string;
  alt?: string;
  onClose: () => void;
}

const ImageLightbox: Component<ImageLightboxProps> = (props) => {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  onMount(() => {
    document.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener("keydown", handleKeyDown);
  });

  return (
    <Portal>
      <div
        class="fixed inset-0 z-[100] flex items-center justify-center bg-black/90 backdrop-blur-sm"
        onClick={(e) => {
          if (e.target === e.currentTarget) props.onClose();
        }}
      >
        <button
          class="absolute top-4 right-4 p-2 text-white/70 hover:text-white hover:bg-white/10 rounded-lg transition-colors z-10"
          onClick={props.onClose}
          title={t("common.close")}
        >
          <X class="w-6 h-6" />
        </button>

        <img
          src={props.src}
          alt={props.alt || ""}
          class="max-w-[90vw] max-h-[90vh] object-contain rounded-lg shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        />
      </div>
    </Portal>
  );
};

export default ImageLightbox;
