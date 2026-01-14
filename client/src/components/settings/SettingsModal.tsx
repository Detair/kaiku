/**
 * Settings Modal
 *
 * Main settings dialog with tabbed navigation.
 */

import { Component, createSignal, For, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Palette, Volume2, Mic } from "lucide-solid";
import AppearanceSettings from "./AppearanceSettings";

interface SettingsModalProps {
  onClose: () => void;
}

type TabId = "appearance" | "audio" | "voice";

interface TabDefinition {
  id: TabId;
  label: string;
  icon: typeof Palette;
}

const tabs: TabDefinition[] = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "audio", label: "Audio", icon: Volume2 },
  { id: "voice", label: "Voice", icon: Mic },
];

const SettingsModal: Component<SettingsModalProps> = (props) => {
  const [activeTab, setActiveTab] = createSignal<TabId>("appearance");

  // Close on escape key
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  // Close on backdrop click
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div class="border border-white/10 rounded-2xl w-[700px] max-h-[600px] flex flex-col shadow-2xl animate-[fadeIn_0.15s_ease-out]" style="background-color: var(--color-surface-layer1)">
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">Settings</h2>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          <div class="flex flex-1 overflow-hidden">
            {/* Sidebar tabs */}
            <div class="w-48 border-r border-white/10 p-3">
              <For each={tabs}>
                {(tab) => {
                  const Icon = tab.icon;
                  return (
                    <button
                      onClick={() => setActiveTab(tab.id)}
                      class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors mb-1"
                      classList={{
                        "bg-accent-primary/20 text-accent-primary":
                          activeTab() === tab.id,
                        "text-text-secondary hover:text-text-primary hover:bg-white/5":
                          activeTab() !== tab.id,
                      }}
                    >
                      <Icon class="w-4 h-4" />
                      <span class="font-medium">{tab.label}</span>
                    </button>
                  );
                }}
              </For>
            </div>

            {/* Content area */}
            <div class="flex-1 overflow-y-auto p-6">
              <Show when={activeTab() === "appearance"}>
                <AppearanceSettings />
              </Show>

              <Show when={activeTab() === "audio"}>
                <div class="text-text-secondary">
                  <h3 class="text-lg font-semibold mb-4 text-text-primary">
                    Audio Settings
                  </h3>
                  <p>Audio device settings coming soon...</p>
                </div>
              </Show>

              <Show when={activeTab() === "voice"}>
                <div class="text-text-secondary">
                  <h3 class="text-lg font-semibold mb-4 text-text-primary">
                    Voice Settings
                  </h3>
                  <p>Voice processing settings coming soon...</p>
                </div>
              </Show>
            </div>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default SettingsModal;
