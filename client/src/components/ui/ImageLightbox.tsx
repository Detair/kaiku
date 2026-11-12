/**
 * ImageLightbox — full-screen dialog for viewing images.
 *
 * A modal dialog (A11Y-1): focus moves to the close button, Escape or a click
 * outside the image closes it, focus returns to the image that opened it.
 */

import { Component } from "solid-js";
import { X } from "lucide-solid";
import { t } from "@/i18n";
import Modal from "./Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

interface ImageLightboxProps {
  src: string;
  alt?: string;
  onClose: () => void;
}

const ImageLightbox: Component<ImageLightboxProps> = (props) => {
  return (
    <Modal
      onClose={() => props.onClose()}
      label={props.alt?.trim() || t("chat.lightbox.label")}
      layer={MODAL_LAYER.dialog + 30}
      backdropClass="fixed inset-0 flex items-center justify-center bg-black/90 backdrop-blur-sm"
      class="relative flex items-center justify-center outline-none"
      testId="image-lightbox"
    >
      <button
        type="button"
        class="fixed top-4 right-4 p-2 min-w-11 min-h-11 flex items-center justify-center text-white/70 hover:text-white hover:bg-white/10 rounded-lg transition-colors z-10"
        onClick={() => props.onClose()}
        title={t("common.close")}
        aria-label={t("common.close")}
      >
        <X class="w-6 h-6" aria-hidden="true" />
      </button>

      <img
        src={props.src}
        alt={props.alt || ""}
        class="max-w-[90vw] max-h-[90vh] object-contain rounded-lg shadow-2xl"
      />
    </Modal>
  );
};

export default ImageLightbox;
