/**
 * Toggle — the one on/off switch of the client.
 *
 * Off: inset track with a visible outline and the knob on the left.
 * On: accent track, contrasting knob on the right carrying a check mark.
 * Colours come from the --color-toggle-* tokens (themes.css), whose contrast
 * is checked per theme in styles/__tests__/contrast.test.ts; pixel themes get
 * square shapes from global.css. Use this instead of hand-built switches.
 */

import { Component, Show } from "solid-js";
import { Check } from "lucide-solid";

export interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** Accessible name when no visible label is linked via `labelledBy`. */
  label?: string;
  /** Id of the element whose text names the switch. */
  labelledBy?: string;
  /** Id of the element that describes the switch (helper text). */
  describedBy?: string;
  disabled?: boolean;
  /** Why the switch cannot be changed; shown as a tooltip when disabled. */
  disabledReason?: string;
  id?: string;
  class?: string;
  "data-testid"?: string;
}

const Toggle: Component<ToggleProps> = (props) => {
  const flip = () => {
    if (!props.disabled) props.onChange(!props.checked);
  };

  return (
    <button
      type="button"
      role="switch"
      id={props.id}
      class={`ui-toggle${props.class ? ` ${props.class}` : ""}`}
      aria-checked={props.checked}
      aria-label={props.labelledBy ? undefined : props.label}
      aria-labelledby={props.labelledBy}
      aria-describedby={props.describedBy}
      data-state={props.checked ? "on" : "off"}
      data-testid={props["data-testid"]}
      disabled={props.disabled}
      title={props.disabled && props.disabledReason ? props.disabledReason : undefined}
      onClick={flip}
      onKeyDown={(e) => {
        // Handle both keys ourselves so the switch behaves the same in every
        // webview (and the native button click does not fire a second time).
        if (e.key === " " || e.key === "Enter") {
          e.preventDefault();
          flip();
        }
      }}
    >
      <span class="ui-toggle-knob" aria-hidden="true">
        <Show when={props.checked}>
          <Check size={12} stroke-width={3.5} data-toggle-check="" />
        </Show>
      </span>
    </button>
  );
};

export default Toggle;
