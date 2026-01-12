/**
 * SettingsModal - Main application settings with tabs
 *
 * Tabbed modal for managing app-wide settings:
 * - Appearance: Theme selection
 * - Audio: Device settings (future)
 * - Voice: Voice settings (future)
 */

import { Component, createSignal, For, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X } from "lucide-solid";
import AppearanceSettings from "./AppearanceSettings";

type SettingsTab = "appearance" | "audio" | "voice";

interface SettingsModalProps {
  onClose: () => void;
}

const SettingsModal: Component<SettingsModalProps> = (props) => {
  const [activeTab, setActiveTab] = createSignal<SettingsTab>("appearance");

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "appearance", label: "Appearance" },
    { id: "audio", label: "Audio" },
    { id: "voice", label: "Voice" },
  ];

  // Handle ESC key to close
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  return (
    <Portal mount={document.body}>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
        onClick={props.onClose}
        onKeyDown={handleKeyDown}
      >
        <div
          class="bg-surface-base border border-white/10 rounded-2xl w-[800px] max-w-[90vw] max-h-[600px] flex flex-col shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">Settings</h2>
            <button
              onClick={props.onClose}
              class="p-2 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
              title="Close settings (ESC)"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Tab Navigation */}
          <div class="flex border-b border-white/10 px-6">
            <For each={tabs}>
              {(tab) => (
                <button
                  onClick={() => setActiveTab(tab.id)}
                  class="px-6 py-3 font-medium capitalize relative transition-colors"
                  classList={{
                    "text-accent-primary": activeTab() === tab.id,
                    "text-text-secondary hover:text-text-primary":
                      activeTab() !== tab.id,
                  }}
                >
                  {tab.label}
                  {/* Active indicator */}
                  <Show when={activeTab() === tab.id}>
                    <div class="absolute bottom-0 left-0 right-0 h-0.5 bg-accent-primary" />
                  </Show>
                </button>
              )}
            </For>
          </div>

          {/* Tab Content */}
          <div class="flex-1 overflow-y-auto p-6">
            <Show when={activeTab() === "appearance"}>
              <AppearanceSettings />
            </Show>
            <Show when={activeTab() === "audio"}>
              <div class="text-text-secondary text-center py-8">
                <div class="text-lg font-semibold mb-2">Audio Settings</div>
                <p>Audio device settings coming soon...</p>
              </div>
            </Show>
            <Show when={activeTab() === "voice"}>
              <div class="text-text-secondary text-center py-8">
                <div class="text-lg font-semibold mb-2">Voice Settings</div>
                <p>Voice-related settings coming soon...</p>
              </div>
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default SettingsModal;
