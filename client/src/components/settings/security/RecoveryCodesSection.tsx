/**
 * Recovery codes (C27): how many are left, "Create new codes" (step-up
 * `recovery_codes.create`) and the new codes shown once.
 */

import { Component, Show, createSignal } from "solid-js";
import { LifeBuoy } from "lucide-solid";
import { plural, t } from "@/i18n";
import { createRecoveryCodes } from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import { isProbation } from "@/stores/auth";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import RecoveryCodesDisplay from "@/components/auth/RecoveryCodesDisplay";
import SectionCard, { errorBox, errorStyle, secondaryButton } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

const RecoveryCodesSection: Component = () => {
  const [codes, setCodes] = createSignal<string[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const left = () => meSecurity()?.recovery_codes_left ?? 0;

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const fresh = await withStepUp(
        { action: "recovery_codes.create", target: "", targetLabel: "" },
        () => createRecoveryCodes(),
      );
      setCodes(fresh);
      void refreshMeSecurity();
    } catch (err) {
      setError(securityErrorText(err, "security.recoveryCodes.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SectionCard
      title={t("security.recoveryCodes.title")}
      icon={<LifeBuoy class="w-5 h-5 text-text-secondary" aria-hidden="true" />}
      testId="security-recovery-codes"
    >
      <Show
        when={codes()}
        fallback={
          <>
            <p class="text-base text-text-primary">
              {plural(left(), { one: "security.recoveryCodes.left.one", other: "security.recoveryCodes.left.other" })}
            </p>
            <p class="text-sm text-text-secondary">{t("security.recoveryCodes.explain")}</p>
            <Show when={!isProbation()}>
              <button type="button" class={secondaryButton} disabled={busy()} onClick={() => void create()}>
                {t("security.recoveryCodes.create")}
              </button>
            </Show>
          </>
        }
      >
        {(fresh) => <RecoveryCodesDisplay codes={fresh()} onDone={() => setCodes(null)} />}
      </Show>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>{error()}</p>
      </Show>
    </SectionCard>
  );
};

export default RecoveryCodesSection;
