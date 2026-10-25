/**
 * Language picker (Appearance settings). Hidden unless the build sets
 * `VITE_FEATURE_LOCALE_SWITCH=true`; the caller gates it.
 */
import { For, Show, type JSX } from "solid-js";
import { LOCALE_NAMES, LOCALES, getLocale, setLocale, t, type Locale } from "@/i18n";

const SELECT_ID = "settings-language-select";

function isLocale(value: string): value is Locale {
  return (LOCALES as readonly string[]).includes(value);
}

export default function LanguageSelect(): JSX.Element {
  return (
    <div class="mt-8 pt-6 border-t border-white/5">
      <label for={SELECT_ID} class="block text-lg font-semibold mb-2 text-text-primary">
        {t("settings.language.label")}
      </label>
      <select
        id={SELECT_ID}
        class="w-full px-4 py-2 bg-surface-base border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors cursor-pointer"
        value={getLocale()}
        onChange={(e) => {
          const value = e.currentTarget.value;
          if (isLocale(value)) void setLocale(value);
        }}
      >
        <For each={LOCALES}>
          {(locale) => (
            <option value={locale} lang={locale}>
              {LOCALE_NAMES[locale]}
            </option>
          )}
        </For>
      </select>
      <Show when={getLocale() !== "en"}>
        <p class="text-sm text-text-secondary mt-2">{t("settings.language.partial")}</p>
      </Show>
    </div>
  );
}
