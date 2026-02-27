/**
 * Appearance Settings
 *
 * Theme selector with visual radio cards.
 */

import { Component, For } from "solid-js";
import { Check, RotateCcw } from "lucide-solid";
import { availableThemes, theme as currentTheme, setTheme, type ThemeDefinition } from "@/stores/theme";
import { updatePreference } from "@/stores/preferences";
import { showToast } from "@/components/ui/Toast";

const AppearanceSettings: Component = () => {
  return (
    <div>
      <h3 class="text-lg font-semibold mb-4 text-text-primary">Theme</h3>
      <p class="text-sm text-text-secondary mb-6">
        Choose your preferred color scheme
      </p>

      <div class="space-y-3">
        <For each={availableThemes}>
          {(theme) => (
            <button
              onClick={() => setTheme(theme.id)}
              class="w-full text-left p-4 rounded-xl border-2 transition-all duration-200"
              classList={{
                "border-accent-primary bg-accent-primary/10":
                  currentTheme() === theme.id,
                "border-white/10 hover:border-accent-primary/50 hover:bg-white/5":
                  currentTheme() !== theme.id,
              }}
            >
              <div class="flex items-start gap-3">
                {/* Radio indicator */}
                <div
                  class="w-5 h-5 rounded-full border-2 flex items-center justify-center flex-shrink-0 mt-0.5 transition-colors"
                  classList={{
                    "border-accent-primary bg-accent-primary":
                      currentTheme() === theme.id,
                    "border-white/30": currentTheme() !== theme.id,
                  }}
                >
                  {currentTheme() === theme.id && (
                    <Check class="w-3 h-3 text-white" />
                  )}
                </div>

                {/* Theme info */}
                <div class="flex-1">
                  <div class="flex items-center gap-2">
                    <span class="font-semibold text-text-primary">
                      {theme.name}
                    </span>
                    <span
                      class="text-xs px-1.5 py-0.5 rounded"
                      classList={{
                        "bg-surface-highlight text-text-secondary":
                          theme.isDark,
                        "bg-amber-100 text-amber-800": !theme.isDark,
                      }}
                    >
                      {theme.isDark ? "Dark" : "Light"}
                    </span>
                  </div>
                  <div class="text-sm text-text-secondary mt-0.5">
                    {theme.description}
                  </div>
                </div>

                {/* Color preview dots */}
                <div class="flex gap-1">
                  <PreviewDot theme={theme} type="surface" />
                  <PreviewDot theme={theme} type="accent" />
                  <PreviewDot theme={theme} type="text" />
                </div>
              </div>
            </button>
          )}
        </For>
      </div>

      {/* Re-run Onboarding */}
      <div class="mt-8 pt-6 border-t border-white/5">
        <h3 class="text-lg font-semibold mb-2 text-text-primary">Setup</h3>
        <p class="text-sm text-text-secondary mb-4">
          Re-run the onboarding wizard to reconfigure your initial settings.
        </p>
        <button
          onClick={() => {
            updatePreference("onboarding_completed", false);
            showToast({
              type: "info",
              title: "Onboarding Reset",
              message: "Close settings to start the onboarding wizard.",
            });
          }}
          class="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-surface-layer2 border border-white/10 text-text-secondary hover:text-text-primary hover:border-white/20 transition-colors"
        >
          <RotateCcw class="w-4 h-4" />
          Re-run Setup Wizard
        </button>
      </div>
    </div>
  );
};

// Color preview dot component
const PreviewDot: Component<{
  theme: ThemeDefinition;
  type: "surface" | "accent" | "text";
}> = (props) => {
  return (
    <div
      class="w-4 h-4 rounded-full border border-white/20"
      style={{ "background-color": props.theme.preview[props.type] }}
    />
  );
};

export default AppearanceSettings;
