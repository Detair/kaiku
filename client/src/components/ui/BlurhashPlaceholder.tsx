/**
 * BlurhashPlaceholder - Renders a blurhash string to a canvas element.
 *
 * Used as a placeholder while images load, providing an instant
 * low-resolution color preview of the image content.
 */

import { Component, createEffect, onCleanup } from "solid-js";
import { decode } from "blurhash";

interface BlurhashPlaceholderProps {
  hash: string;
  width: number;
  height: number;
  class?: string;
}

const BlurhashPlaceholder: Component<BlurhashPlaceholderProps> = (props) => {
  let canvasRef: HTMLCanvasElement | undefined;

  createEffect(() => {
    if (!canvasRef || !props.hash) return;

    try {
      // Decode at a small fixed size for performance (32x32 is plenty for a blur)
      const renderWidth = 32;
      const renderHeight = Math.max(
        1,
        Math.round((renderWidth * props.height) / Math.max(props.width, 1)),
      );

      const pixels = decode(props.hash, renderWidth, renderHeight);
      const ctx = canvasRef.getContext("2d");
      if (!ctx) return;

      canvasRef.width = renderWidth;
      canvasRef.height = renderHeight;

      const imageData = ctx.createImageData(renderWidth, renderHeight);
      imageData.data.set(pixels);
      ctx.putImageData(imageData, 0, 0);
    } catch {
      // Silently fail — the image will load normally without a placeholder
    }
  });

  onCleanup(() => {
    canvasRef = undefined;
  });

  return (
    <canvas
      ref={canvasRef}
      class={props.class}
      style={{ "image-rendering": "auto" }}
    />
  );
};

export default BlurhashPlaceholder;
