/**
 * EnrollmentIssuerPanel — make a phone (or a browser's passkey) a key of
 * this account (F5a, SM2). Issues a setup code (`own`: after the step-up
 * `enrollment.issue`; also used by the request and registration issuers),
 * shows its link as a QR code, waits for the claim (the `enrollment_updated`
 * hint or a poll every 3 s), names the device "calling itself …" and takes
 * the 6 digits it shows. The link itself is never shown as text.
 */

import { Component, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { plural, t } from "@/i18n";
import {
  cancelEnrollment,
  confirmEnrollment,
  getEnrollment,
  issueEnrollment,
  type EnrollmentIssuer,
} from "@/lib/tauri/enrollments";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { withStepUp } from "@/lib/stepUp";
import { ENROLLMENT_UPDATED_EVENT } from "@/lib/types/events";
import type { Enrollment, EnrollmentIssued } from "@/lib/types";
import { offerSignOutOthers } from "@/stores/security";
import QrCodeImage from "@/components/auth/QrCodeImage";
import CodeEntry from "@/components/auth/CodeEntry";
import MinutesLeft from "@/components/auth/login/MinutesLeft";
import { errorBox, errorStyle, primaryButton, secondaryButton } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

export interface EnrollmentIssuerPanelProps {
  issuer: EnrollmentIssuer;
  onDone: (e: Enrollment) => void;
  onCancel: () => void;
}

/** Poll for the claim this often while waiting (the WS hint is faster). */
const POLL_MS = 3_000;

const EnrollmentIssuerPanel: Component<EnrollmentIssuerPanelProps> = (props) => {
  const [issued, setIssued] = createSignal<EnrollmentIssued | null>(null);
  const [enrollment, setEnrollment] = createSignal<Enrollment | null>(null);
  const [code, setCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [failed, setFailed] = createSignal(false);
  const [copied, setCopied] = createSignal(false);
  let timer: ReturnType<typeof setInterval> | undefined;
  let disposed = false;

  const stopPolling = () => {
    if (timer) clearInterval(timer);
    timer = undefined;
  };

  const state = () => enrollment()?.state ?? "issued";
  const finished = () => failed() || ["done", "cancelled", "expired", "failed"].includes(state());

  const refresh = async () => {
    const id = issued()?.enrollment_id;
    if (!id || disposed || finished()) return;
    try {
      const e = await getEnrollment(props.issuer, id);
      if (disposed) return;
      setEnrollment(e);
      if (["done", "cancelled", "expired", "failed"].includes(e.state)) stopPolling();
    } catch (err) {
      console.warn("[Enrollment] Refresh failed:", err instanceof Error ? err.message : err);
    }
  };

  const onHint = (event: Event) => {
    const detail = (event as CustomEvent<{ enrollment_id?: string }>).detail;
    if (detail?.enrollment_id && detail.enrollment_id === issued()?.enrollment_id) void refresh();
  };

  onMount(async () => {
    window.addEventListener(ENROLLMENT_UPDATED_EVENT, onHint);
    setBusy(true);
    try {
      const issue = () => issueEnrollment(props.issuer);
      const result =
        props.issuer.kind === "own"
          ? await withStepUp(
              { action: "enrollment.issue", target: "", targetLabel: t("security.setupPhone.title") },
              issue,
            )
          : await issue();
      if (disposed) return;
      setIssued(result);
      timer = setInterval(() => void refresh(), POLL_MS);
    } catch (err) {
      setError(securityErrorText(err, "security.setupPhone.issueFailed") ?? null);
      if (err instanceof Error && err.name === "StepUpCancelled") props.onCancel();
    } finally {
      setBusy(false);
    }
  });

  onCleanup(() => {
    disposed = true;
    stopPolling();
    window.removeEventListener(ENROLLMENT_UPDATED_EVENT, onHint);
  });

  const copyLink = async () => {
    const url = issued()?.setup_url;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  const canShare = () => typeof navigator !== "undefined" && typeof navigator.share === "function";
  const shareLink = async () => {
    const url = issued()?.setup_url;
    if (!url) return;
    try {
      await navigator.share({ title: t("security.setupPhone.shareTitle"), url });
    } catch {
      // The user closed the share sheet.
    }
  };

  const confirm = async () => {
    const id = issued()?.enrollment_id;
    if (!id) return;
    setBusy(true);
    setError(null);
    try {
      const e = await confirmEnrollment(props.issuer, id, code());
      setEnrollment(e);
      setCode("");
      if (e.state === "done") {
        stopPolling();
        if (props.issuer.kind === "own") offerSignOutOthers();
        props.onDone(e);
      }
    } catch (err) {
      setCode("");
      if (err instanceof HttpError && codeIs(err.code, "ENROLL_CODE_WRONG")) {
        const left = typeof err.details.tries_left === "number" ? err.details.tries_left : 0;
        setError(plural(left, { one: "security.setupPhone.wrongCode.one", other: "security.setupPhone.wrongCode.other" }));
      } else if (err instanceof HttpError && codeIs(err.code, "ENROLLMENT_FAILED")) {
        stopPolling();
        setFailed(true);
      } else {
        setError(securityErrorText(err, "security.setupPhone.confirmFailed"));
      }
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    const id = issued()?.enrollment_id;
    stopPolling();
    if (id && !finished()) {
      try {
        await cancelEnrollment(props.issuer, id);
      } catch (err) {
        console.warn("[Enrollment] Cancel failed:", err instanceof Error ? err.message : err);
      }
    }
    props.onCancel();
  };

  const label = () => enrollment()?.claimed?.label ?? "";

  return (
    <div class="space-y-4">
      <Switch>
        <Match when={failed() || state() === "failed"}>
          <p class="text-base text-text-primary" role="status">{t("security.setupPhone.failed")}</p>
        </Match>
        <Match when={state() === "expired"}>
          <p class="text-base text-text-primary" role="status">{t("security.setupPhone.expired")}</p>
        </Match>
        <Match when={state() === "cancelled"}>
          <p class="text-base text-text-primary" role="status">{t("security.setupPhone.cancelled")}</p>
        </Match>
        <Match when={state() === "done"}>
          <p class="text-lg font-medium text-text-primary" role="status">
            {t("security.setupPhone.done", { label: label() })}
          </p>
        </Match>
        <Match when={state() === "claimed"}>
          <p class="text-lg font-medium text-text-primary">{t("security.setupPhone.claimed", { label: label() })}</p>
          <p class="text-base text-text-primary">{t("security.setupPhone.typeCode")}</p>
          <CodeEntry
            length={6}
            value={code()}
            onInput={setCode}
            onSubmit={() => void confirm()}
            disabled={busy()}
            label={t("security.setupPhone.codeLabel")}
          />
        </Match>
        <Match when={issued()}>
          {(i) => (
            <>
              <ol class="space-y-2 text-base text-text-primary list-decimal list-inside">
                <li>{t("security.setupPhone.step1")}</li>
                <li>{t("security.setupPhone.step2")}</li>
                <li>{t("security.setupPhone.step3")}</li>
              </ol>
              <div class="flex justify-center">
                <QrCodeImage payload={i().setup_url} alt={t("security.setupPhone.qrAlt")} size={240} />
              </div>
              <MinutesLeft expiresAt={i().expires_at} />
              <div class="flex flex-wrap justify-center gap-2">
                <button type="button" class={secondaryButton} onClick={() => void copyLink()}>
                  {copied() ? t("common.copied") : t("security.setupPhone.copyLink")}
                </button>
                <Show when={canShare()}>
                  <button type="button" class={secondaryButton} onClick={() => void shareLink()}>
                    {t("security.setupPhone.shareLink")}
                  </button>
                </Show>
              </div>
            </>
          )}
        </Match>
      </Switch>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>
          {error()}
        </p>
      </Show>
      <div class="flex justify-end">
        <Show
          when={finished()}
          fallback={
            <button type="button" class={secondaryButton} onClick={() => void cancel()}>
              {t("common.cancel")}
            </button>
          }
        >
          <button type="button" class={primaryButton} onClick={() => props.onCancel()}>
            {t("common.close")}
          </button>
        </Show>
      </div>
    </div>
  );
};

export default EnrollmentIssuerPanel;
