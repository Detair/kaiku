import { Component, createSignal, JSX, splitProps } from "solid-js";
import { Eye, EyeOff } from "lucide-solid";

type PasswordInputProps = Omit<JSX.InputHTMLAttributes<HTMLInputElement>, "type" | "children">;

/**
 * Password field with visibility toggle.
 * Accepts all `<input>` props except `type` (managed internally).
 * Renders a wrapper `<div>` around the native `<input>`.
 * The toggle button uses `tabIndex={-1}` to stay out of the form's
 * tab order — keyboard users interact via the browser's built-in
 * password reveal or autocomplete instead.
 */
const PasswordInput: Component<PasswordInputProps> = (props) => {
  const [local, inputProps] = splitProps(props, ["class"]);
  const [visible, setVisible] = createSignal(false);

  return (
    <div class="relative">
      <input
        {...inputProps}
        type={visible() ? "text" : "password"}
        class={`${local.class ?? ""} pr-10`}
      />
      <button
        type="button"
        class="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-text-secondary hover:text-text-primary transition-colors rounded"
        onClick={() => setVisible(!visible())}
        aria-label={visible() ? "Hide password" : "Show password"}
        tabIndex={-1}
      >
        {visible() ? <EyeOff class="w-4 h-4" /> : <Eye class="w-4 h-4" />}
      </button>
    </div>
  );
};

export default PasswordInput;
