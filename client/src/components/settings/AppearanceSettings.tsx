/**
 * AppearanceSettings - Theme selection UI
 *
 * Displays theme options as radio cards with descriptions.
 * Updates theme in real-time via theme store.
 */

import { Component, For, Show } from "solid-js";
import { themeState, setTheme } from "@/stores/theme";

const AppearanceSettings: Component = () => {
  return (
    <div>
      <h3 class="text-lg font-semibold text-text-primary mb-2">Theme</h3>
      <p class="text-sm text-text-secondary mb-4">
        Choose your preferred color scheme. Changes apply instantly.
      </p>

      <div class="space-y-3">
        <For each={themeState.availableThemes}>
          {(theme) => (
            <button
              onClick={() => setTheme(theme.id)}
              class="w-full text-left p-4 rounded-xl border-2 transition-all duration-200"
              classList={{
                "border-accent-primary bg-accent-primary/10":
                  themeState.currentTheme === theme.id,
                "border-white/10 hover:border-accent-primary/50":
                  themeState.currentTheme !== theme.id,
              }}
            >
              <div class="flex items-start gap-3">
                {/* Radio indicator */}
                <div
                  class="w-5 h-5 rounded-full border-2 flex-shrink-0 flex items-center justify-center transition-all"
                  classList={{
                    "border-accent-primary bg-accent-primary":
                      themeState.currentTheme === theme.id,
                    "border-white/30": themeState.currentTheme !== theme.id,
                  }}
                >
                  <Show when={themeState.currentTheme === theme.id}>
                    <div class="w-2 h-2 bg-surface-base rounded-full" />
                  </Show>
                </div>

                {/* Theme info */}
                <div class="flex-1">
                  <div class="font-semibold text-text-primary mb-1">
                    {theme.name}
                  </div>
                  <div class="text-sm text-text-secondary">
                    {theme.description}
                  </div>
                </div>

                {/* Color preview swatches */}
                <div class="flex gap-1.5 items-center">
                  <div
                    class="w-5 h-5 rounded border border-white/10"
                    style={{
                      "background-color":
                        theme.id === "focused-hybrid"
                          ? "#1E1E2E"
                          : theme.id === "solarized-dark"
                            ? "#002b36"
                            : "#fdf6e3",
                    }}
                    title="Background"
                  />
                  <div
                    class="w-5 h-5 rounded border border-white/10"
                    style={{
                      "background-color":
                        theme.id === "focused-hybrid"
                          ? "#88C0D0"
                          : "#268bd2",
                    }}
                    title="Accent"
                  />
                </div>
              </div>
            </button>
          )}
        </For>
      </div>
    </div>
  );
};

export default AppearanceSettings;
