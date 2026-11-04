/**
 * Recovery contact (C28, Part C): the person who gets this account's
 * important notices and may cancel a fake request. Choose (step-up
 * `recovery_contact.set`/user id), remove (`recovery_contact.set`/""), and
 * for people who chose you, "I will help <name>" (full session, no step-up).
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { Users } from "lucide-solid";
import { t } from "@/i18n";
import { formatShortDate } from "@/i18n/format";
import {
  confirmContactFor,
  listContactFor,
  removeRecoveryContact,
  setRecoveryContact,
} from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import { currentUser, isProbation } from "@/stores/auth";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import SectionCard, { dangerButton, errorBox, errorStyle, secondaryButton } from "./SectionCard";
import UserPicker, { type PickedUser } from "./UserPicker";
import { securityErrorText } from "./securityErrors";

const RecoveryContactSection: Component = () => {
  const [picking, setPicking] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [helping, { refetch }] = createResource(async () => {
    try {
      return await listContactFor();
    } catch {
      return [];
    }
  });
  const contact = () => meSecurity()?.recovery_contact ?? null;

  const run = async (job: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await job();
      void refreshMeSecurity();
    } catch (err) {
      setError(securityErrorText(err, "security.contact.failed"));
    } finally {
      setBusy(false);
    }
  };

  const choose = (u: PickedUser) => {
    setPicking(false);
    void run(() =>
      withStepUp({ action: "recovery_contact.set", target: u.user_id, targetLabel: u.display_name }, () =>
        setRecoveryContact(u.user_id),
      ),
    );
  };

  const remove = () =>
    void run(() =>
      withStepUp({ action: "recovery_contact.set", target: "", targetLabel: "" }, () => removeRecoveryContact()),
    );

  const confirmHelp = (userId: string) =>
    void run(async () => {
      await confirmContactFor(userId);
      await refetch();
    });

  return (
    <SectionCard
      title={t("security.contact.title")}
      icon={<Users class="w-5 h-5 text-text-secondary" aria-hidden="true" />}
      testId="security-recovery-contact"
    >
      <p class="text-sm text-text-secondary">{t("security.contact.explain")}</p>
      <Show
        when={contact()}
        fallback={<p class="text-base text-text-primary">{t("security.contact.none")}</p>}
      >
        {(c) => (
          <div class="p-3 rounded-lg bg-surface-layer2 space-y-1">
            <p class="text-base font-medium text-text-primary">{c().display_name}</p>
            <p class="text-sm text-text-secondary">
              {c().confirmed_at
                ? c().counts_from
                  ? t("security.contact.countsFrom", { date: formatShortDate(c().counts_from!) })
                  : t("security.contact.confirmed")
                : t("security.contact.waiting")}
            </p>
          </div>
        )}
      </Show>
      <Show when={!isProbation()}>
        <Show
          when={picking()}
          fallback={
            <div class="flex flex-wrap gap-2">
              <button type="button" class={secondaryButton} disabled={busy()} onClick={() => setPicking(true)}>
                {t("security.contact.choose")}
              </button>
              <Show when={contact()}>
                <button type="button" class={dangerButton} disabled={busy()} onClick={remove}>
                  {t("security.contact.remove")}
                </button>
              </Show>
            </div>
          }
        >
          <UserPicker
            onPick={choose}
            onCancel={() => setPicking(false)}
            exclude={[currentUser()?.id ?? "", contact()?.user_id ?? ""]}
          />
        </Show>
      </Show>
      <Show when={(helping() ?? []).length > 0}>
        <div class="space-y-2 pt-2 border-t border-white/10">
          <h5 class="text-base font-medium text-text-primary">{t("security.contact.chosenBy")}</h5>
          <ul class="space-y-2">
            <For each={helping()}>
              {(p) => (
                <li class="flex flex-wrap items-center gap-3">
                  <span class="flex-1 text-base text-text-primary">{p.display_name}</span>
                  <Show
                    when={!p.confirmed_at}
                    fallback={<span class="text-sm text-text-secondary">{t("security.contact.youHelp")}</span>}
                  >
                    <Show when={!isProbation()}>
                      <button type="button" class={secondaryButton} disabled={busy()} onClick={() => confirmHelp(p.user_id)}>
                        {t("security.contact.willHelp", { name: p.display_name })}
                      </button>
                    </Show>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>{error()}</p>
      </Show>
    </SectionCard>
  );
};

export default RecoveryContactSection;
