/**
 * CodeEntry — the 6-digit enrolment code (C26), for older users too: digits
 * only (paste strips spaces), shown in groups ("123 456"), corrected with
 * Backspace or the visible "Delete" key, a large on-screen keypad, and
 * "Confirm" enabled at six digits. It never submits by itself.
 */

import { Component, For, createUniqueId } from "solid-js";
import { t } from "@/i18n";

/** "123456" → "123 456". */
export function formatCode(digits: string): string {
  const d = digits.replace(/\D+/g, "");
  return d.length <= 3 ? d : `${d.slice(0, 3)} ${d.slice(3)}`;
}

export interface CodeEntryProps {
  length: 6;
  value: string;
  onInput: (digits: string) => void;
  onSubmit: () => void;
  disabled?: boolean;
  label: string;
}

const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

const CodeEntry: Component<CodeEntryProps> = (props) => {
  const id = createUniqueId();
  const clean = (raw: string) => raw.replace(/\D+/g, "").slice(0, props.length);
  const complete = () => props.value.length === props.length;

  const press = (digit: string) => {
    if (props.disabled || props.value.length >= props.length) return;
    props.onInput(clean(props.value + digit));
  };
  const erase = () => {
    if (props.disabled) return;
    props.onInput(props.value.slice(0, -1));
  };

  return (
    <form
      class="space-y-4"
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
        if (complete() && !props.disabled) props.onSubmit();
      }}
    >
      <label for={id} class="block text-base font-medium text-text-primary">
        {props.label}
      </label>
      <input
        id={id}
        type="text"
        inputmode="numeric"
        autocomplete="one-time-code"
        class="input-field text-center font-mono text-4xl tracking-[0.3em] min-h-16"
        value={formatCode(props.value)}
        disabled={props.disabled}
        onInput={(e) => {
          const digits = clean(e.currentTarget.value);
          e.currentTarget.value = formatCode(digits);
          props.onInput(digits);
        }}
        onKeyDown={(e) => {
          // Enter never submits an incomplete code.
          if (e.key === "Enter" && !complete()) e.preventDefault();
        }}
      />
      <div class="grid grid-cols-3 gap-2" role="group" aria-label={t("security.code.keypad")}>
        <For each={KEYS.slice(0, 9)}>
          {(k) => (
            <button
              type="button"
              class="min-h-14 rounded-lg bg-surface-layer2 hover:bg-white/10 text-2xl font-semibold text-text-primary state-disabled"
              disabled={props.disabled}
              aria-label={t("security.code.digit", { digit: k })}
              onClick={() => press(k)}
            >
              {k}
            </button>
          )}
        </For>
        <button
          type="button"
          class="min-h-14 rounded-lg bg-surface-layer2 hover:bg-white/10 text-base font-medium text-text-primary state-disabled"
          disabled={props.disabled || props.value.length === 0}
          onClick={erase}
        >
          {t("security.code.delete")}
        </button>
        <button
          type="button"
          class="min-h-14 rounded-lg bg-surface-layer2 hover:bg-white/10 text-2xl font-semibold text-text-primary state-disabled"
          disabled={props.disabled}
          aria-label={t("security.code.digit", { digit: "0" })}
          onClick={() => press("0")}
        >
          0
        </button>
        <button type="submit" class="btn-primary min-h-14 text-base" disabled={props.disabled || !complete()}>
          {t("security.code.confirm")}
        </button>
      </div>
    </form>
  );
};

export default CodeEntry;
