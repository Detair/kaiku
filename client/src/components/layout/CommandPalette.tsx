/**
 * CommandPalette - Quick Action Interface
 *
 * Ctrl+K / Cmd+K quick search and command execution.
 * Inspired by Linear, VSCode, and Raycast.
 *
 * Features:
 * - Global keyboard trigger (Ctrl/Cmd + K)
 * - Fuzzy search for channels, users, and commands
 * - Keyboard navigation (Up/Down/Enter/Esc)
 * - Command execution (> prefix for actions like "> Mute")
 * - Optimized search (memoized item list, only filters on keystroke)
 *
 * UI:
 * - Centered modal dialog
 * - Large borderless input
 * - List of results with icons
 */

import {
  Component,
  createSignal,
  createEffect,
  createMemo,
  For,
  Show,
  onCleanup,
} from "solid-js";
import { Hash, Volume2, Search, Mic, MicOff, VolumeX } from "lucide-solid";
import { channelsState, selectChannel } from "@/stores/channels";
import { setShowGlobalSearch } from "@/stores/search";
import { toggleMute, toggleDeafen, voiceState } from "@/stores/voice";
import type { Channel } from "@/lib/types";

interface CommandItem {
  id: string;
  type: "channel" | "user" | "command";
  label: string;
  icon: Component;
  action: () => void;
}

const CommandPalette: Component = () => {
  const [isOpen, setIsOpen] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;

  // Generate command items
  const getCommandItems = (): CommandItem[] => {
    const items: CommandItem[] = [];

    // Add channels
    channelsState.channels.forEach((channel: Channel) => {
      items.push({
        id: channel.id,
        type: "channel",
        label: `# ${channel.name}`,
        icon: channel.channel_type === "voice" ? Volume2 : Hash,
        action: () => {
          selectChannel(channel.id);
          setIsOpen(false);
        },
      });
    });

    // Search Everywhere command (always available)
    items.push({
      id: "cmd-search-everywhere",
      type: "command",
      label: "Search Everywhere",
      icon: Search,
      action: () => {
        setIsOpen(false);
        setShowGlobalSearch(true);
      },
    });

    // Add commands (if query starts with >)
    if (query().startsWith(">")) {
      items.push({
        id: "cmd-mute",
        type: "command",
        label: voiceState.muted ? "Unmute Microphone" : "Mute Microphone",
        icon: voiceState.muted ? MicOff : Mic,
        action: () => {
          toggleMute();
          setIsOpen(false);
        },
      });
      items.push({
        id: "cmd-deafen",
        type: "command",
        label: voiceState.deafened ? "Undeafen" : "Deafen",
        icon: VolumeX,
        action: () => {
          toggleDeafen();
          setIsOpen(false);
        },
      });
    }

    return items;
  };

  // All available command items (only recalculates when channels change)
  const allCommandItems = createMemo(() => getCommandItems());

  // Filter items based on query (optimized: only filters, doesn't rebuild list)
  const filteredItems = createMemo(() => {
    const items = allCommandItems();
    const searchQuery = query()
      .toLowerCase()
      .replace(/^[>#@]/, "");

    if (!searchQuery) return items;

    return items.filter((item) =>
      item.label.toLowerCase().includes(searchQuery),
    );
  });

  // Keyboard event handler
  const handleKeyDown = (e: KeyboardEvent) => {
    // Open palette with Ctrl+K / Cmd+K
    if ((e.ctrlKey || e.metaKey) && e.key === "k") {
      e.preventDefault();
      setIsOpen(true);
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef?.focus(), 10);
      return;
    }

    // Handle navigation when palette is open
    if (!isOpen()) return;

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        setIsOpen(false);
        break;

      case "ArrowDown":
        e.preventDefault();
        setSelectedIndex((prev) =>
          prev < filteredItems().length - 1 ? prev + 1 : prev,
        );
        break;

      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((prev) => (prev > 0 ? prev - 1 : prev));
        break;

      case "Enter":
        e.preventDefault();
        const selected = filteredItems()[selectedIndex()];
        if (selected) {
          selected.action();
        }
        break;
    }
  };

  // Register global keyboard listener
  createEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    onCleanup(() => window.removeEventListener("keydown", handleKeyDown));
  });

  // Reset selected index when query changes
  createEffect(() => {
    query(); // Track dependency
    setSelectedIndex(0);
  });

  // Close on backdrop click
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      setIsOpen(false);
    }
  };

  return (
    <Show when={isOpen()}>
      {/* Backdrop */}
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm z-[100] flex items-start justify-center pt-[20vh]"
        onClick={handleBackdropClick}
      >
        {/* Command Palette Dialog */}
        <div
          class="w-[600px] border border-white/10 shadow-2xl rounded-xl overflow-hidden animate-slide-up"
          style="background-color: var(--color-surface-layer2)"
        >
          {/* Input */}
          <div class="border-b border-white/5">
            <input
              ref={inputRef}
              type="text"
              placeholder="Search channels, users, or type > for commands..."
              class="w-full px-6 py-4 bg-transparent text-xl text-text-input outline-none placeholder:text-text-secondary/40"
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
          </div>

          {/* Results */}
          <div class="max-h-[400px] overflow-y-auto">
            <Show
              when={filteredItems().length > 0}
              fallback={
                <div class="px-6 py-8 text-center text-text-secondary">
                  No results found
                </div>
              }
            >
              <For each={filteredItems()}>
                {(item, index) => (
                  <button
                    class="w-full px-6 py-3 flex items-center gap-3 transition-colors text-left"
                    classList={{
                      "bg-surface-highlight": index() === selectedIndex(),
                      "hover:bg-surface-highlight/50":
                        index() !== selectedIndex(),
                    }}
                    onClick={() => item.action()}
                    onMouseEnter={() => setSelectedIndex(index())}
                  >
                    <div class="w-5 h-5 text-text-secondary flex-shrink-0">
                      <item.icon />
                    </div>
                    <span class="text-text-primary font-medium">
                      {item.label}
                    </span>
                    <Show when={item.type === "command"}>
                      <span class="ml-auto text-xs text-text-secondary">
                        Command
                      </span>
                    </Show>
                  </button>
                )}
              </For>
            </Show>
          </div>

          {/* Footer Hint */}
          <div class="px-6 py-2 border-t border-white/5 bg-surface-base">
            <div class="flex items-center gap-4 text-xs text-text-secondary">
              <div>
                <kbd class="px-1.5 py-0.5 bg-surface-layer2 rounded border border-white/10">
                  ↑↓
                </kbd>
                <span class="ml-1">Navigate</span>
              </div>
              <div>
                <kbd class="px-1.5 py-0.5 bg-surface-layer2 rounded border border-white/10">
                  Enter
                </kbd>
                <span class="ml-1">Select</span>
              </div>
              <div>
                <kbd class="px-1.5 py-0.5 bg-surface-layer2 rounded border border-white/10">
                  Esc
                </kbd>
                <span class="ml-1">Close</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default CommandPalette;
