/**
 * FileInput — themed replacement for the native "Choose File / no file
 * selected" control: a real button plus the chosen file's name.
 *
 * The native <input type="file"> stays in the DOM (visually hidden) so
 * keyboard, screen readers and form semantics keep working; the button just
 * opens it. The parent owns the selected file (`fileName`) so it can clear it.
 */

import { Component, Show, createUniqueId } from "solid-js";
import { Upload } from "lucide-solid";
import { t } from "@/i18n";

export interface FileInputProps {
  onFile: (file: File) => void;
  /** Name of the currently chosen file, or null/empty when none. */
  fileName?: string | null;
  accept?: string;
  disabled?: boolean;
  /** Button text; defaults to "Choose file". */
  buttonLabel?: string;
  /** Accessible name of the hidden native input. */
  label?: string;
  "data-testid"?: string;
}

const FileInput: Component<FileInputProps> = (props) => {
  const nameId = createUniqueId();
  let input: HTMLInputElement | undefined;

  return (
    <div class="flex items-center gap-3 min-w-0">
      <input
        ref={input}
        type="file"
        class="sr-only"
        tabIndex={-1}
        accept={props.accept}
        disabled={props.disabled}
        aria-label={props.label ?? props.buttonLabel ?? t("common.fileInput.choose")}
        data-testid={props["data-testid"]}
        onChange={(e) => {
          const file = e.currentTarget.files?.[0];
          if (file) props.onFile(file);
          // Allow picking the same file again after the parent cleared it.
          e.currentTarget.value = "";
        }}
      />
      <button
        type="button"
        disabled={props.disabled}
        aria-describedby={nameId}
        onClick={() => input?.click()}
        class="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg bg-surface-layer1 text-text-primary border border-border-solid hover:bg-surface-highlight transition-colors flex-shrink-0 state-disabled"
      >
        <Upload size={14} aria-hidden="true" />
        {props.buttonLabel ?? t("common.fileInput.choose")}
      </button>
      <span
        id={nameId}
        class="text-sm truncate min-w-0"
        classList={{
          "text-text-primary": !!props.fileName,
          "text-text-secondary": !props.fileName,
        }}
      >
        <Show when={props.fileName} fallback={t("common.fileInput.none")}>
          {props.fileName}
        </Show>
      </span>
    </div>
  );
};

export default FileInput;
