/**
 * TauriVideoFrame — Renders video frames received from the Rust backend.
 *
 * In Tauri mode there are no browser MediaStreamTrack objects. Instead, the
 * Rust backend (after PR D's VP8 decoder) emits decoded I420 frames over a
 * Tauri `Channel<DecodedFrame>` returned from the `subscribe_video_frames`
 * command. This component creates the channel, subscribes for the requested
 * `stream_id`, and uploads each frame into a `<canvas>` via WebGL2 using
 * `NativeYuvRenderer`.
 *
 * Phase 1 scope: only remote screen shares are decoded server-side. Webcam
 * tiles in Tauri mode (props.streamId === "webcam" / "default") fall back
 * to a "Video stream active" placeholder until a later PR wires the webcam
 * pipeline through the same channel.
 */

import { Component, Show, onMount, onCleanup, createSignal } from "solid-js";
import {
  NativeYuvRenderer,
  type DecodedFrame,
} from "@/lib/voice/nativeVideoRenderer";
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

  // Phase 1 only decodes remote screen shares server-side. The Rust
  // `video_frame_sinks` map uses the screen share's UUID as the key (see
  // `commands/voice.rs`). The webcam call site passes the literal "webcam"
  // string and there is no matching subscriber, so we render the placeholder.
  const isScreenShare = () =>
    props.streamId !== "webcam" && props.streamId !== "default";

  onMount(async () => {
    if (!isTauri || !canvasRef || !isScreenShare()) return;

    try {
      renderer = new NativeYuvRenderer(canvasRef);
    } catch (err) {
      console.error("[TauriVideoFrame] renderer init failed:", err);
      setError(err instanceof Error ? err.message : String(err));
      return;
    }

    try {
      const { invoke, Channel } = await import("@tauri-apps/api/core");
      const channel = new Channel<DecodedFrame>();

      channel.onmessage = (frame) => {
        // Defensive: the subscriber pipeline is keyed by stream_id but a
        // future change could fan multiple streams through one channel.
        if (frame.stream_id !== props.streamId) return;
        if (!renderer) return;
        try {
          renderer.renderFrame(frame);
          if (!hasFrames()) setHasFrames(true);
        } catch (err) {
          console.error("[TauriVideoFrame] render failed:", err);
          setError(err instanceof Error ? err.message : String(err));
        }
      };

      await invoke("subscribe_video_frames", {
        streamId: props.streamId,
        onFrame: channel,
      });
    } catch (err) {
      console.error("[TauriVideoFrame] subscribe failed:", err);
      setError(err instanceof Error ? err.message : String(err));
    }
  });

  onCleanup(() => {
    renderer?.dispose();
    renderer = null;
    // The Tauri Channel itself is GC'd along with this component; the Rust
    // sender side drops once the frontend stops draining (best-effort).
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
