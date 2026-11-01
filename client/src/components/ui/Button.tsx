/**
 * Button — shared button with a readable disabled state and a visible reason.
 *
 * Disabled buttons use the .state-disabled / .state-disabled-quiet styles
 * from global.css (readable label, dashed outline, not-allowed cursor)
 * instead of fading out. Pass `disabledReason` whenever the button is
 * disabled because something is missing ("Enter a channel name"): it is shown
 * as helper text next to the button, as a tooltip and as the button's
 * accessible description.
 */

import { Component, JSX, Show, createUniqueId, splitProps } from "solid-js";

export type ButtonVariant = "primary" | "secondary" | "danger" | "ghost";
export type ButtonSize = "sm" | "md";

export interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Why the button is disabled. Ignored while enabled. */
  disabledReason?: string | false | null;
  /** Render the reason as text before the button (default) or only as a tooltip. */
  showReason?: boolean;
}

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent-primary text-on-accent hover:bg-primary-hover font-semibold state-disabled",
  secondary:
    "bg-surface-layer2 text-text-primary border border-border-solid hover:bg-surface-highlight font-medium state-disabled",
  danger: "bg-accent-danger text-on-danger hover:bg-accent-danger/90 font-semibold state-disabled",
  ghost: "text-text-primary hover:bg-surface-layer2 state-disabled-quiet",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "px-3 py-1.5 text-xs",
  md: "px-5 py-2 text-sm",
};

const Button: Component<ButtonProps> = (props) => {
  const [local, rest] = splitProps(props, [
    "variant",
    "size",
    "disabledReason",
    "showReason",
    "class",
    "type",
    "children",
    "title",
    "aria-describedby",
  ]);
  const hintId = createUniqueId();
  const reason = () => (props.disabled && local.disabledReason ? local.disabledReason : null);
  const showHint = () => reason() !== null && local.showReason !== false;

  return (
    <>
      <Show when={showHint()}>
        <span id={hintId} class="ui-disabled-reason text-xs text-text-secondary">
          {reason()}
        </span>
      </Show>
      <button
        {...rest}
        type={local.type ?? "button"}
        title={reason() ?? local.title}
        aria-describedby={showHint() ? hintId : local["aria-describedby"]}
        class={`inline-flex items-center justify-center gap-1.5 rounded-lg whitespace-nowrap transition-colors ${
          SIZES[local.size ?? "md"]
        } ${VARIANTS[local.variant ?? "primary"]}${local.class ? ` ${local.class}` : ""}`}
      >
        {local.children}
      </button>
    </>
  );
};

export default Button;
