/**
 * TauriVideoFrame — Renders video frames received from the Rust backend.
 *
 * In Tauri mode there are no browser MediaStreamTrack objects. Instead, the
 * Rust backend will (once the VP8 decoder is implemented) emit decoded frames
 * as `voice:video_frame` events. This component listens for those events and
 * paints them onto a <canvas>.
 *
 * For now the VP8 decoder is a stub, so this shows a placeholder text. The
 * component structure is in place so that wiring up real frames later only
 * requires emitting the event from Rust.
 */

import { Component, Show, onMount, onCleanup, createSignal } from "solid-js";

interface TauriVideoFrameProps {
  userId: string;
  streamId: string;
}

const TauriVideoFrame: Component<TauriVideoFrameProps> = (props) => {
  let canvasRef: HTMLCanvasElement | undefined;
  const [hasFrames, setHasFrames] = createSignal(false);

  onMount(async () => {
    if (!("__TAURI__" in window)) return;
    const { listen } = await import("@tauri-apps/api/event");

    const unlisten = await listen<{
      user_id: string;
      stream_id: string;
      width: number;
      height: number;
      data: string; // base64-encoded RGBA or JPEG frame
    }>("voice:video_frame", (event) => {
      if (
        event.payload.user_id !== props.userId ||
        event.payload.stream_id !== props.streamId
      )
        return;
      if (!canvasRef) return;

      setHasFrames(true);
      const img = new Image();
      img.onload = () => {
        if (!canvasRef) return;
        canvasRef.width = event.payload.width;
        canvasRef.height = event.payload.height;
        const ctx = canvasRef.getContext("2d");
        ctx?.drawImage(img, 0, 0);
      };
      img.src = `data:image/jpeg;base64,${event.payload.data}`;
    });

    onCleanup(() => unlisten());
  });

  return (
    <div class="w-full h-full flex items-center justify-center bg-surface-layer2 rounded-lg">
      <Show
        when={hasFrames()}
        fallback={
          <span class="text-text-secondary text-sm">Video stream active</span>
        }
      >
        <canvas ref={canvasRef} class="w-full h-full object-contain" />
      </Show>
    </div>
  );
};

export default TauriVideoFrame;
