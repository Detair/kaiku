/**
 * Frame for one security-settings section: heading, body, and for a
 * chat-only session the note that changes need a passkey sign-in (the
 * section then renders read-only, B.4).
 */

import { Component, JSX, Show } from "solid-js";
import { t } from "@/i18n";
import { isProbation } from "@/stores/auth";

export const ReadOnlyNote: Component = () => (
  <p class="text-sm text-text-secondary">{t("security.settings.readOnly")}</p>
);

export interface SectionCardProps {
  title: string;
  icon?: JSX.Element;
  children: JSX.Element;
  testId?: string;
}

const SectionCard: Component<SectionCardProps> = (props) => (
  <section class="bg-surface-base rounded-xl p-4 space-y-3" data-testid={props.testId}>
    <div class="flex items-center gap-3">
      {props.icon}
      <h4 class="text-base font-semibold text-text-primary">{props.title}</h4>
    </div>
    {props.children}
    <Show when={isProbation()}>
      <ReadOnlyNote />
    </Show>
  </section>
);

export default SectionCard;

/** Shared button classes (≥ 48 px high, readable text, CLAUDE.md contrast). */
export const primaryButton = "btn-primary min-h-12 text-base";
export const secondaryButton =
  "min-h-12 px-4 rounded-lg border border-white/10 bg-white/5 hover:bg-white/10 text-base text-text-primary state-disabled";
export const dangerButton =
  "min-h-12 px-4 rounded-lg border border-status-danger/50 hover:bg-status-danger/10 text-base text-text-primary state-disabled";
export const errorBox = "p-3 rounded-md text-sm";
export const errorStyle = {
  "background-color": "var(--color-error-bg)",
  border: "1px solid var(--color-error-border)",
  color: "var(--color-error-text)",
} as const;
