/**
 * Appearance Settings
 *
 * Theme selector with visual radio cards ("Same as my device" first), and the
 * text size.
 */

import { Component, For, Show } from "solid-js";
import { Check, RotateCcw } from "lucide-solid";
import {
  pickerThemes,
  storedTheme,
  theme as shownTheme,
  followSystemTheme,
  setTheme,
  setFollowSystemTheme,
  fontSize,
  setFontSize,
  availableThemes,
  type FontSize,
  type ThemeDefinition,
} from "@/stores/theme";
import { updatePreference } from "@/stores/preferences";
import { showToast } from "@/components/ui/Toast";
import { isLocaleSwitchEnabled } from "@/lib/featureFlags";
import LanguageSelect from "./LanguageSelect";
import { t } from "@/i18n";

const FONT_SIZES: { id: FontSize; label: () => string }[] = [
  { id: "default", label: () => t("settings.appearance.fontSize.default") },
  { id: "large", label: () => t("settings.appearance.fontSize.large") },
  { id: "xlarge", label: () => t("settings.appearance.fontSize.xlarge") },
];

/**
 * Arrow keys for a radio group (WAI-ARIA radio pattern): one Tab stop, the
 * arrows move to the next or previous option and choose it.
 */
const onRadioGroupKeyDown = (e: KeyboardEvent & { currentTarget: HTMLElement }) => {
  const radios = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]'));
  const index = radios.indexOf(e.target as HTMLElement);
  if (index < 0) return;
  let next: number;
  switch (e.key) {
    case "ArrowDown":
    case "ArrowRight":
      next = (index + 1) % radios.length;
      break;
    case "ArrowUp":
    case "ArrowLeft":
      next = (index - 1 + radios.length) % radios.length;
      break;
    case "Home":
      next = 0;
      break;
    case "End":
      next = radios.length - 1;
      break;
    default:
      return;
  }
  e.preventDefault();
  radios[next].focus();
  radios[next].click();
};

const AppearanceSettings: Component = () => {
  const shownName = () => availableThemes.find((th) => th.id === shownTheme())?.name ?? "";
  // The group keeps one Tab stop: the chosen option, or the first one when a
  // theme outside the list is stored.
  const themeListed = () => pickerThemes().some((th) => th.id === storedTheme());

  return (
    <div>
      <h3 class="text-lg font-semibold mb-4 text-text-primary">{t("settings.appearance.theme")}</h3>
      <p class="text-sm text-text-secondary mb-6">
        {t("settings.appearance.themeHint")}
      </p>

      <div
        class="space-y-3"
        role="radiogroup"
        aria-label={t("settings.appearance.theme")}
        onKeyDown={onRadioGroupKeyDown}
      >
        {/* Same as system */}
        <button
          type="button"
          role="radio"
          aria-checked={followSystemTheme()}
          tabIndex={followSystemTheme() || !themeListed() ? 0 : -1}
          onClick={() => setFollowSystemTheme(true)}
          class="w-full text-left p-4 rounded-xl border-2 transition-all duration-200"
          classList={{
            "border-accent-primary bg-accent-primary/20": followSystemTheme(),
            "border-border-solid hover:border-accent-primary/50 hover:bg-surface-highlight":
              !followSystemTheme(),
          }}
        >
          <div class="flex items-start gap-3">
            <RadioDot checked={followSystemTheme()} />
            <div class="flex-1">
              <span class="font-semibold text-text-primary">
                {t("settings.appearance.themeSystem")}
              </span>
              <div class="text-sm text-text-secondary mt-0.5">
                {t("settings.appearance.themeSystemDesc")}
              </div>
              <Show when={followSystemTheme()}>
                <div class="text-sm text-text-secondary mt-1">
                  {t("settings.appearance.themeSystemNow", { name: shownName() })}
                </div>
              </Show>
            </div>
          </div>
        </button>

        <For each={pickerThemes()}>
          {(theme) => {
            const selected = () => !followSystemTheme() && storedTheme() === theme.id;
            return (
              <button
                type="button"
                role="radio"
                aria-checked={selected()}
                tabIndex={selected() ? 0 : -1}
                onClick={() => setTheme(theme.id)}
                class="w-full text-left p-4 rounded-xl border-2 transition-all duration-200"
                classList={{
                  "border-accent-primary bg-accent-primary/20": selected(),
                  "border-border-solid hover:border-accent-primary/50 hover:bg-surface-highlight":
                    !selected(),
                }}
              >
                <div class="flex items-start gap-3">
                  <RadioDot checked={selected()} />

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
                        {theme.isDark ? t("settings.appearance.dark") : t("settings.appearance.light")}
                      </span>
                    </div>
                    <div class="text-sm text-text-secondary mt-0.5">
                      {theme.description}
                    </div>
                  </div>

                  {/* Color preview dots */}
                  <div class="flex gap-1" aria-hidden="true">
                    <PreviewDot theme={theme} type="surface" />
                    <PreviewDot theme={theme} type="accent" />
                    <PreviewDot theme={theme} type="text" />
                  </div>
                </div>
              </button>
            );
          }}
        </For>
      </div>

      {/* Text size */}
      <div class="mt-8 pt-6 border-t border-border-default">
        <h3 class="text-lg font-semibold mb-2 text-text-primary">{t("settings.appearance.fontSize")}</h3>
        <p class="text-sm text-text-secondary mb-4">
          {t("settings.appearance.fontSizeHint")}
        </p>
        <div
          class="flex flex-wrap gap-2"
          role="radiogroup"
          aria-label={t("settings.appearance.fontSize")}
          onKeyDown={onRadioGroupKeyDown}
        >
          <For each={FONT_SIZES}>
            {(size) => (
              <button
                type="button"
                role="radio"
                aria-checked={fontSize() === size.id}
                tabIndex={fontSize() === size.id ? 0 : -1}
                onClick={() => setFontSize(size.id)}
                class="px-4 py-2 rounded-lg border-2 text-sm font-medium transition-colors"
                classList={{
                  "border-accent-primary bg-accent-primary/20 text-text-primary":
                    fontSize() === size.id,
                  "border-border-solid text-text-secondary hover:border-accent-primary/50 hover:text-text-primary":
                    fontSize() !== size.id,
                }}
              >
                {size.label()}
              </button>
            )}
          </For>
        </div>
        <p class="mt-4 text-base text-text-primary" data-testid="font-size-sample">
          {t("settings.appearance.fontSizeSample")}
        </p>
      </div>

      <Show when={isLocaleSwitchEnabled()}><LanguageSelect /></Show>

      {/* Re-run Onboarding */}
      <div class="mt-8 pt-6 border-t border-border-default">
        <h3 class="text-lg font-semibold mb-2 text-text-primary">{t("settings.appearance.setup")}</h3>
        <p class="text-sm text-text-secondary mb-4">
          {t("settings.appearance.setupHint")}
        </p>
        <button
          onClick={() => {
            updatePreference("onboarding_completed", false);
            showToast({
              type: "info",
              title: t("settings.appearance.onboardingResetTitle"),
              message: t("settings.appearance.onboardingResetMessage"),
            });
          }}
          class="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-surface-layer2 border border-border-solid text-text-secondary hover:text-text-primary hover:border-accent-primary/50 transition-colors"
        >
          <RotateCcw class="w-4 h-4" />
          {t("settings.appearance.rerunSetup")}
        </button>
      </div>
    </div>
  );
};

// Radio indicator
const RadioDot: Component<{ checked: boolean }> = (props) => (
  <div
    class="w-5 h-5 rounded-full border-2 flex items-center justify-center flex-shrink-0 mt-0.5 transition-colors"
    classList={{
      "border-accent-primary bg-accent-primary": props.checked,
      "border-border-input": !props.checked,
    }}
    aria-hidden="true"
  >
    {props.checked && <Check class="w-3 h-3 text-on-accent" />}
  </div>
);

// Color preview dot component
const PreviewDot: Component<{
  theme: ThemeDefinition;
  type: "surface" | "accent" | "text";
}> = (props) => {
  return (
    <div
      class="w-4 h-4 rounded-full border border-border-solid"
      style={{ "background-color": props.theme.preview[props.type] }}
    />
  );
};

export default AppearanceSettings;
