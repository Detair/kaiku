/**
 * RecoveryCodesDisplay — new recovery codes, shown once (13 characters,
 * grouped 5-5-3 for reading aloud and writing down). Each works once and
 * only starts an account recovery (F6), never a sign-in.
 */

import { Component, For, createSignal } from "solid-js";
import { t } from "@/i18n";

/** "ABCDEFGHJKMNP" → "ABCDE FGHJK MNP". */
export function groupRecoveryCode(code: string): string {
  const clean = code.replace(/[\s-]+/g, "").toUpperCase();
  return clean.match(/.{1,5}/g)?.join(" ") ?? clean;
}

export interface RecoveryCodesDisplayProps {
  codes: string[];
  onDone: () => void;
}

const RecoveryCodesDisplay: Component<RecoveryCodesDisplayProps> = (props) => {
  const [copied, setCopied] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.codes.map(groupRecoveryCode).join("\n"));
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div class="space-y-4">
      <p class="text-base text-text-primary">{t("security.recoveryCodes.writeDown")}</p>
      <ol class="grid grid-cols-1 sm:grid-cols-2 gap-2" aria-label={t("security.recoveryCodes.listLabel")}>
        <For each={props.codes}>
          {(code) => (
            <li class="font-mono text-lg tracking-wider text-text-primary bg-surface-layer2 rounded-lg px-3 py-2 text-center">
              {groupRecoveryCode(code)}
            </li>
          )}
        </For>
      </ol>
      <div class="flex flex-wrap gap-2 justify-end">
        <button
          type="button"
          class="min-h-12 px-4 rounded-lg border border-white/10 bg-white/5 hover:bg-white/10 text-base text-text-primary"
          onClick={() => void copy()}
        >
          {copied() ? t("common.copied") : t("common.copy")}
        </button>
        <button type="button" class="btn-primary min-h-12 text-base" onClick={() => props.onDone()}>
          {t("security.recoveryCodes.done")}
        </button>
      </div>
    </div>
  );
};

export default RecoveryCodesDisplay;
