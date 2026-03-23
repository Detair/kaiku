/**
 * VoiceTileStrip — Sidebar (vertical) or bottom bar (horizontal) strip of
 * unfocused tiles shown during focus mode.
 */

import { Component, For } from "solid-js";
import VoiceTile from "./VoiceTile";
import type { TileData } from "./VoiceTile";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface VoiceTileStripProps {
  tiles: TileData[];
  orientation: "vertical" | "horizontal";
  onTileClick: (tileId: string) => void;
  poppedOutStreams: Set<string>;
  onPopOut: (streamId: string) => void;
  onBringBack: (streamId: string) => void;
  onStopShare?: (streamId: string) => void;
  localUserId?: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function getTileId(tile: TileData): string {
  return tile.type === "screen_share" ? `screen:${tile.streamId}` : tile.userId;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const VoiceTileStrip: Component<VoiceTileStripProps> = (props) => {
  const isVertical = () => props.orientation === "vertical";

  return (
    <div
      class={
        isVertical()
          ? "flex flex-col gap-1.5 w-40"
          : "flex gap-1.5 overflow-x-auto h-20"
      }
    >
      <For each={props.tiles}>
        {(tile) => (
          <div
            class={isVertical() ? "flex-1" : "w-20 flex-shrink-0"}
          >
            <VoiceTile
              tile={tile}
              onClick={() => props.onTileClick(getTileId(tile))}
              size="small"
              poppedOut={
                tile.type === "screen_share"
                  ? props.poppedOutStreams.has(tile.streamId)
                  : undefined
              }
              onPopOut={
                tile.type === "screen_share"
                  ? () => props.onPopOut(tile.streamId)
                  : undefined
              }
              onBringBack={
                tile.type === "screen_share"
                  ? () => props.onBringBack(tile.streamId)
                  : undefined
              }
              onStopShare={
                tile.type === "screen_share" && props.onStopShare && tile.userId === props.localUserId
                  ? () => props.onStopShare!(tile.streamId)
                  : undefined
              }
            />
          </div>
        )}
      </For>
    </div>
  );
};

export default VoiceTileStrip;
