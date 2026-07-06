/**
 * KeyboardShortcutsDialog - Keyboard Shortcuts Help Overlay
 *
 * Displays all available keyboard shortcuts organized by category.
 * Triggered via Ctrl+/, ? key, or the /? slash command.
 *
 * Features:
 * - Full-screen semi-transparent overlay with centered modal
 * - Shortcut categories: General, Voice, Chat
 * - Styled <kbd> key badges
 * - Dismiss via Escape key or backdrop click
 */

import { Component, For, onCleanup, onMount } from "solid-js";
import { X } from "lucide-solid";
import { appSettings } from "@/stores/settings";
import { keyCodeToLabel } from "@/lib/pttManager";

interface ShortcutEntry {
  keys: string[];
  description: string;
}

interface ShortcutCategory {
  title: string;
  shortcuts: ShortcutEntry[];
}

function getShortcutCategories(): ShortcutCategory[] {
  const settings = appSettings();
  const voiceShortcuts: ShortcutEntry[] = [
    { keys: ["Ctrl", "Shift", "M"], description: "Toggle microphone mute" },
    { keys: ["Ctrl", "Shift", "D"], description: "Toggle deafen" },
  ];

  if (settings?.voice.push_to_talk && settings.voice.push_to_talk_key) {
    voiceShortcuts.push({
      keys: [keyCodeToLabel(settings.voice.push_to_talk_key)],
      description: "Push to Talk (hold)",
    });
  }

  if (settings?.voice.push_to_mute && settings.voice.push_to_mute_key) {
    voiceShortcuts.push({
      keys: [keyCodeToLabel(settings.voice.push_to_mute_key)],
      description: "Push to Mute (hold)",
    });
  }

  return [
    {
      title: "General",
      shortcuts: [
        { keys: ["Ctrl", "K"], description: "Open command palette" },
        { keys: ["Ctrl", "Shift", "F"], description: "Toggle global search" },
        { keys: ["Ctrl", "/"], description: "Toggle this dialog" },
      ],
    },
    { title: "Voice", shortcuts: voiceShortcuts },
    {
      title: "Chat",
      shortcuts: [
        { keys: ["Ctrl", "F"], description: "Search in channel" },
        { keys: ["Enter"], description: "Send message" },
        { keys: ["Shift", "Enter"], description: "New line" },
        { keys: ["Ctrl", "B"], description: "Bold text" },
        { keys: ["Ctrl", "I"], description: "Italic text" },
        { keys: ["Ctrl", "E"], description: "Inline code" },
      ],
    },
  ];
}

interface KeyboardShortcutsDialogProps {
  onClose: () => void;
}

const KeyboardShortcutsDialog: Component<KeyboardShortcutsDialogProps> = (props) => {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
  };

  onMount(() => {
    window.addEventListener("keydown", handleKeyDown, { capture: true });
  });

  onCleanup(() => {
    window.removeEventListener("keydown", handleKeyDown, { capture: true });
  });

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <div
      class="fixed inset-0 bg-black/60 backdrop-blur-sm z-[100] flex items-center justify-center"
      onClick={handleBackdropClick}
    >
      <div
        class="w-[560px] max-h-[80vh] border border-white/10 shadow-2xl rounded-xl overflow-hidden flex flex-col"
        style={{"background-color":"var(--color-surface-layer2)"}}
      >
        {/* Header */}
        <div class="px-6 py-4 border-b border-white/5 flex items-center justify-between">
          <h2 class="text-lg font-semibold text-text-primary">Keyboard Shortcuts</h2>
          <button
            onClick={() => props.onClose()}
            class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
            title="Close"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="flex-1 overflow-y-auto px-6 py-4 space-y-6">
          <For each={getShortcutCategories()}>
            {(category) => (
              <div>
                <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wider mb-3">
                  {category.title}
                </h3>
                <div class="space-y-2">
                  <For each={category.shortcuts}>
                    {(shortcut) => (
                      <div class="flex items-center justify-between py-1.5">
                        <span class="text-sm text-text-primary">{shortcut.description}</span>
                        <div class="flex items-center gap-1">
                          <For each={shortcut.keys}>
                            {(key, index) => (
                              <>
                                {index() > 0 && (
                                  <span class="text-xs text-text-secondary mx-0.5">+</span>
                                )}
                                <kbd class="inline-flex items-center justify-center min-w-[24px] h-6 px-1.5 text-xs font-medium text-text-primary bg-surface-base rounded border border-white/10 shadow-sm">
                                  {key}
                                </kbd>
                              </>
                            )}
                          </For>
                        </div>
                      </div>
                    )}
                  </For>
                </div>
              </div>
            )}
          </For>
        </div>

        {/* Footer */}
        <div class="px-6 py-3 border-t border-white/5 bg-surface-base">
          <div class="flex items-center justify-center gap-1 text-xs text-text-secondary">
            <span>Press</span>
            <kbd class="px-1.5 py-0.5 bg-surface-layer2 rounded border border-white/10">
              Ctrl+/
            </kbd>
            <span>or</span>
            <kbd class="px-1.5 py-0.5 bg-surface-layer2 rounded border border-white/10">
              ?
            </kbd>
            <span>to toggle this dialog</span>
          </div>
        </div>
      </div>
    </div>
  );
};

export default KeyboardShortcutsDialog;
