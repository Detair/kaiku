/**
 * RecoveryCodeInput — a recovery code as typed or pasted: case, spaces and
 * dashes do not matter (13 base32 characters, shown grouped).
 */

import { Component, createUniqueId } from "solid-js";
import { groupRecoveryCode } from "./RecoveryCodesDisplay";

/** Upper-case, spaces and dashes removed ("abcde-fghjk mnp" → "ABCDEFGHJKMNP"). */
export function normalizeRecoveryCode(input: string): string {
  return input.replace(/[\s-]+/g, "").toUpperCase();
}

export interface RecoveryCodeInputProps {
  value: string;
  onInput: (normalized: string) => void;
  label: string;
  disabled?: boolean;
}

const RecoveryCodeInput: Component<RecoveryCodeInputProps> = (props) => {
  const id = createUniqueId();
  return (
    <div class="space-y-1">
      <label for={id} class="block text-base font-medium text-text-primary">
        {props.label}
      </label>
      <input
        id={id}
        class="input-field font-mono text-xl tracking-wider min-h-12 uppercase"
        autocomplete="off"
        autocapitalize="characters"
        spellcheck={false}
        value={groupRecoveryCode(props.value)}
        disabled={props.disabled}
        onInput={(e) => props.onInput(normalizeRecoveryCode(e.currentTarget.value).slice(0, 32))}
      />
    </div>
  );
};

export default RecoveryCodeInput;
