/**
 * Channels Store
 *
 * Manages channel list and selection state.
 */

import { createStore } from "solid-js/store";
import type { Channel } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// Channels state interface
interface ChannelsState {
  channels: Channel[];
  selectedChannelId: string | null;
  isLoading: boolean;
  error: string | null;
}

// Create the store
const [channelsState, setChannelsState] = createStore<ChannelsState>({
  channels: [],
  selectedChannelId: null,
  isLoading: false,
  error: null,
});

// Derived state

export const selectedChannel = () =>
  channelsState.channels.find((c) => c.id === channelsState.selectedChannelId);

export const textChannels = () =>
  channelsState.channels
    .filter((c) => c.channel_type === "text")
    .sort((a, b) => a.position - b.position);

export const voiceChannels = () =>
  channelsState.channels
    .filter((c) => c.channel_type === "voice")
    .sort((a, b) => a.position - b.position);

// Actions

/**
 * Load channels from server.
 */
export async function loadChannels(): Promise<void> {
  setChannelsState({ isLoading: true, error: null });

  try {
    const channels = await tauri.getChannels();
    setChannelsState({
      channels,
      isLoading: false,
      error: null,
    });

    // Auto-select first text channel if none selected
    if (!channelsState.selectedChannelId && channels.length > 0) {
      const firstText = channels.find((c) => c.channel_type === "text");
      if (firstText) {
        setChannelsState({ selectedChannelId: firstText.id });
      }
    }
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load channels:", error);
    setChannelsState({ isLoading: false, error });
  }
}

/**
 * Select a channel.
 */
export function selectChannel(channelId: string): void {
  setChannelsState({ selectedChannelId: channelId });
}

/**
 * Clear channel selection.
 */
export function clearSelection(): void {
  setChannelsState({ selectedChannelId: null });
}

/**
 * Find a channel by ID.
 */
export function getChannel(channelId: string): Channel | undefined {
  return channelsState.channels.find((c) => c.id === channelId);
}

/**
 * Create a new channel.
 */
export async function createChannel(
  name: string,
  channelType: "text" | "voice",
  topic?: string
): Promise<Channel> {
  const channel = await tauri.createChannel(name, channelType, topic);
  setChannelsState("channels", (prev) => [...prev, channel]);
  return channel;
}

// Export the store for reading
export { channelsState };
