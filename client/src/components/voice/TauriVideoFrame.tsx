/**
 * TauriVideoFrame — Renders video frames received from the Rust backend.
 *
 * In Tauri mode there are no browser MediaStreamTrack objects. Instead, the
 * Rust backend decodes VP8 and sends each I420 frame as raw bytes over a
 * Tauri channel given to `subscribe_video_frames` (undone by
 * `unsubscribe_video_frames` on unmount). This component subscribes for the
 * `stream_id`, and uploads each frame into a `<canvas>` via WebGL2 using
 * `NativeYuvRenderer`.
 *
 * Phase 1 scope: only remote screen shares are decoded server-side. Webcam
 * tiles in Tauri mode (props.streamId === "webcam" / "default") fall back
 * to a "Video stream active" placeholder until a later PR wires the webcam
 * pipeline through the same channel.
 */

import { Component, Show, onMount, onCleanup, createSignal } from "solid-js";
import { NativeYuvRenderer, parseNativeFrame } from "@/lib/voice/nativeVideoRenderer";
import { isTauri } from "@/lib/tauri/common";

interface TauriVideoFrameProps {
  userId: string;
  streamId: string;
}

const TauriVideoFrame: Component<TauriVideoFrameProps> = (props) => {
  let canvasRef: HTMLCanvasElement | undefined;
  const [hasFrames, setHasFrames] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let renderer: NativeYuvRenderer | null = null;
  let disposed = false;
  let subscription: {
    invoke: (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
    id: number;
  } | null = null;

  // Phase 1 only decodes remote screen shares server-side. The Rust
  // `video_frame_sinks` map uses the screen share's UUID as the key (see
  // `commands/voice.rs`). The webcam call site passes the literal "webcam"
  // string and there is no matching subscriber, so we render the placeholder.
  const isScreenShare = () =>
    props.streamId !== "webcam" && props.streamId !== "default";

  onMount(async () => {
    if (!isTauri() || !canvasRef || !isScreenShare()) return;

    try {
      renderer = new NativeYuvRenderer(canvasRef);
    } catch (err) {
      console.error("[TauriVideoFrame] renderer init failed:", err);
      setError(err instanceof Error ? err.message : String(err));
      return;
    }

    try {
      const { invoke, Channel } = await import("@tauri-apps/api/core");
      // Raw bytes per frame (no JSON), parsed into views of one buffer.
      const channel = new Channel<ArrayBuffer>();
      channel.onmessage = (raw) => {
        if (!renderer || !(raw instanceof ArrayBuffer)) return;
        const frame = parseNativeFrame(raw, props.streamId);
        if (!frame) return;
        try {
          renderer.renderFrame(frame);
          if (!hasFrames()) setHasFrames(true);
        } catch (err) {
          console.error("[TauriVideoFrame] render failed:", err);
          setError(err instanceof Error ? err.message : String(err));
        }
      };

      const id = await invoke<number>("subscribe_video_frames", {
        streamId: props.streamId,
        onFrame: channel,
      });
      if (disposed) {
        void invoke("unsubscribe_video_frames", { streamId: props.streamId, subscriptionId: id });
      } else {
        subscription = { invoke, id };
      }
    } catch (err) {
      console.error("[TauriVideoFrame] subscribe failed:", err);
      setError(err instanceof Error ? err.message : String(err));
    }
  });

  onCleanup(() => {
    disposed = true;
    renderer?.dispose();
    renderer = null;
    // The view goes: the decoder stops sending to it (and stops decoding
    // when no view of the stream is left).
    if (subscription) {
      void subscription.invoke("unsubscribe_video_frames", {
        streamId: props.streamId,
        subscriptionId: subscription.id,
      });
      subscription = null;
    }
  });

  return (
    <div class="w-full h-full flex items-center justify-center bg-surface-layer2 rounded-lg">
      <Show
        when={isScreenShare()}
        fallback={
          <span class="text-text-secondary text-sm">Video stream active</span>
        }
      >
        <Show
          when={!error()}
          fallback={
            <span class="text-text-secondary text-sm">
              Video error: {error()}
            </span>
          }
        >
          <canvas
            ref={canvasRef}
            class="w-full h-full object-contain"
            classList={{ hidden: !hasFrames() }}
          />
          <Show when={!hasFrames()}>
            <span class="text-text-secondary text-sm">
              Video stream active
            </span>
          </Show>
        </Show>
      </Show>
    </div>
  );
};

export default TauriVideoFrame;
