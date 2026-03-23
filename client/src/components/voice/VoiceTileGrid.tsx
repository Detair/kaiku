/**
 * VoiceTileGrid — Responsive tile grid with focus mode for voice channels.
 *
 * Grid mode: fills the container with an optimal square-fit layout.
 * Focus mode: one tile is enlarged, the rest are shown in a side/bottom strip.
 * Auto-focuses remote screen shares when they start.
 */

import {
  Component,
  For,
  Show,
  createSignal,
  createMemo,
  createEffect,
  onMount,
  onCleanup,
} from "solid-js";
import type { VoiceParticipant } from "@/lib/types";
import type { ScreenShareInfo } from "@/lib/webrtc/types";
import { authState } from "@/stores/auth";
import VoiceTile from "./VoiceTile";
import type { TileData } from "./VoiceTile";
import VoiceTileStrip from "./VoiceTileStrip";
import { calculateGrid } from "./tileLayout";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function getTileId(tile: TileData): string {
  return tile.type === "screen_share" ? `screen:${tile.streamId}` : tile.userId;
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface VoiceTileGridProps {
  participants: VoiceParticipant[];
  screenShares: ScreenShareInfo[];
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const VoiceTileGrid: Component<VoiceTileGridProps> = (props) => {
  const [focusedTileId, setFocusedTileId] = createSignal<string | null>(null);
  const [containerSize, setContainerSize] = createSignal({ width: 0, height: 0 });
  const [poppedOutStreams, setPoppedOutStreams] = createSignal<Set<string>>(new Set());

  // ---- Tile list construction ----
  const tiles = createMemo<TileData[]>(() => {
    const participantTiles: TileData[] = props.participants.map((p) => ({
      type: "participant" as const,
      userId: p.user_id,
      displayName: p.display_name ?? p.username ?? "",
      username: p.username ?? "",
      muted: p.muted,
      deafened: false,
      speaking: p.speaking,
    }));

    const screenTiles: TileData[] = props.screenShares.map((s) => ({
      type: "screen_share" as const,
      streamId: s.stream_id,
      userId: s.user_id,
      username: s.username,
    }));

    return [...participantTiles, ...screenTiles];
  });

  // ---- Auto-focus logic ----
  let prevScreenShareCount = 0;

  createEffect(() => {
    const shares = props.screenShares;
    const currentCount = shares.length;
    const localUserId = authState.user?.id;

    // Screen shares went from 0 to 1+ — auto-focus first remote share
    if (prevScreenShareCount === 0 && currentCount > 0) {
      const firstRemote = shares.find((s) => s.user_id !== localUserId);
      if (firstRemote) {
        setFocusedTileId(`screen:${firstRemote.stream_id}`);
      }
    }

    // Focused screen share stopped — pick next remote share or clear
    const focused = focusedTileId();
    if (focused?.startsWith("screen:")) {
      const focusedStreamId = focused.slice("screen:".length);
      const stillExists = shares.some((s) => s.stream_id === focusedStreamId);
      if (!stillExists) {
        const nextRemote = shares.find((s) => s.user_id !== localUserId);
        setFocusedTileId(nextRemote ? `screen:${nextRemote.stream_id}` : null);
      }
    }

    prevScreenShareCount = currentCount;
  });

  // ---- View mode ----
  const viewMode = () => (focusedTileId() ? "focus" : "grid");

  // ---- Container sizing via ResizeObserver ----
  let containerRef: HTMLDivElement | undefined;

  onMount(() => {
    if (!containerRef) return;
    const observer = new ResizeObserver((entries) => {
      const { width, height } = entries[0].contentRect;
      setContainerSize({ width, height });
    });
    observer.observe(containerRef);
    onCleanup(() => observer.disconnect());
  });

  // ---- Grid layout ----
  const layout = () =>
    calculateGrid(tiles().length, containerSize().width, containerSize().height);

  // ---- Click handling ----
  const handleTileClick = (tileId: string) => {
    setFocusedTileId((prev) => (prev === tileId ? null : tileId));
  };

  // ---- Keyboard ----
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      setFocusedTileId(null);
    }
  };

  // ---- Pop-out wiring (stubs until screenSharePopOut utility is created) ----
  const handlePopOut = (streamId: string) => {
    setPoppedOutStreams((prev) => {
      const next = new Set(prev);
      next.add(streamId);
      return next;
    });
    // TODO: call popOut() from screenSharePopOut when Task 5 is complete
  };

  const handleBringBack = (streamId: string) => {
    setPoppedOutStreams((prev) => {
      const next = new Set(prev);
      next.delete(streamId);
      return next;
    });
    // TODO: call bringBack() from screenSharePopOut when Task 5 is complete
  };

  // Cleanup pop-outs on unmount
  onCleanup(() => {
    // TODO: call closeAll() from screenSharePopOut when Task 5 is complete
    setPoppedOutStreams(new Set());
  });

  // ---- Focus mode helpers ----
  const focusedTile = () => {
    const id = focusedTileId();
    if (!id) return undefined;
    return tiles().find((t) => getTileId(t) === id);
  };

  const remainingTiles = () => {
    const id = focusedTileId();
    if (!id) return tiles();
    return tiles().filter((t) => getTileId(t) !== id);
  };

  const stripOrientation = (): "vertical" | "horizontal" =>
    remainingTiles().length <= 6 ? "vertical" : "horizontal";

  // ---- Render ----
  return (
    <div
      ref={containerRef}
      class="w-full h-full p-2 outline-none"
      tabindex="-1"
      onKeyDown={handleKeyDown}
    >
      {/* Empty state */}
      <Show when={tiles().length === 0}>
        <div class="flex items-center justify-center h-full text-text-muted text-sm">
          Waiting for others...
        </div>
      </Show>

      {/* Grid mode */}
      <Show when={tiles().length > 0 && viewMode() === "grid"}>
        <div
          class="w-full h-full grid justify-center items-center"
          style={{
            "grid-template-columns": `repeat(${layout().cols}, ${layout().tileWidth}px)`,
            gap: "8px",
          }}
        >
          <For each={tiles()}>
            {(tile) => (
              <div
                style={{
                  width: `${layout().tileWidth}px`,
                  height: `${layout().tileHeight}px`,
                }}
              >
                <VoiceTile
                  tile={tile}
                  onClick={() => handleTileClick(getTileId(tile))}
                  size="normal"
                  poppedOut={
                    tile.type === "screen_share"
                      ? poppedOutStreams().has(tile.streamId)
                      : undefined
                  }
                  onPopOut={
                    tile.type === "screen_share"
                      ? () => handlePopOut(tile.streamId)
                      : undefined
                  }
                  onBringBack={
                    tile.type === "screen_share"
                      ? () => handleBringBack(tile.streamId)
                      : undefined
                  }
                />
              </div>
            )}
          </For>
        </div>
      </Show>

      {/* Focus mode */}
      <Show when={tiles().length > 0 && viewMode() === "focus"}>
        <div
          class={
            stripOrientation() === "vertical"
              ? "flex h-full gap-2"
              : "flex flex-col h-full gap-2"
          }
        >
          {/* Focused tile */}
          <div class="flex-1 min-h-0 min-w-0">
            <Show when={focusedTile()}>
              {(tile) => (
                <VoiceTile
                  tile={tile()}
                  onClick={() => handleTileClick(getTileId(tile()))}
                  size="normal"
                  poppedOut={
                    tile().type === "screen_share"
                      ? poppedOutStreams().has(
                          (tile() as Extract<TileData, { type: "screen_share" }>).streamId,
                        )
                      : undefined
                  }
                  onPopOut={
                    tile().type === "screen_share"
                      ? () =>
                          handlePopOut(
                            (tile() as Extract<TileData, { type: "screen_share" }>).streamId,
                          )
                      : undefined
                  }
                  onBringBack={
                    tile().type === "screen_share"
                      ? () =>
                          handleBringBack(
                            (tile() as Extract<TileData, { type: "screen_share" }>).streamId,
                          )
                      : undefined
                  }
                />
              )}
            </Show>
          </div>

          {/* Strip of remaining tiles */}
          <Show when={remainingTiles().length > 0}>
            <VoiceTileStrip
              tiles={remainingTiles()}
              orientation={stripOrientation()}
              onTileClick={handleTileClick}
              poppedOutStreams={poppedOutStreams()}
              onPopOut={handlePopOut}
              onBringBack={handleBringBack}
            />
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default VoiceTileGrid;
