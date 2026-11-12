/**
 * Report Modal
 *
 * Lets people report a person or a message to the admins. A modal dialog
 * (A11Y-1): focus moves in, Tab stays inside, Escape closes, focus returns.
 */

import { Component, createSignal, createUniqueId, For, Show } from "solid-js";
import { X, Flag } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { t, type MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";
import Modal from "@/components/ui/Modal";
import Button from "@/components/ui/Button";

export interface ReportTarget {
  userId: string;
  username: string;
  messageId?: string;
}

interface ReportModalProps {
  target: ReportTarget;
  onClose: () => void;
}

type Category = tauri.CreateReportRequest["category"];

const CATEGORIES: { value: Category; label: MessageKey }[] = [
  { value: "harassment", label: "moderation.report.category.harassment" },
  { value: "spam", label: "moderation.report.category.spam" },
  { value: "inappropriate_content", label: "moderation.report.category.inappropriateContent" },
  { value: "impersonation", label: "moderation.report.category.impersonation" },
  { value: "other", label: "moderation.report.category.other" },
];

const ReportModal: Component<ReportModalProps> = (props) => {
  const titleId = createUniqueId();
  const reasonId = createUniqueId();
  const detailsId = createUniqueId();
  const [category, setCategory] = createSignal<Category>("harassment");
  const [description, setDescription] = createSignal("");
  const [isSubmitting, setIsSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [success, setSuccess] = createSignal(false);

  const targetType = () =>
    (props.target.messageId ? "message" : "user") as "user" | "message";

  const handleSubmit = async () => {
    setIsSubmitting(true);
    setError(null);

    try {
      await tauri.createReport({
        target_type: targetType(),
        target_user_id: props.target.userId,
        target_message_id: props.target.messageId,
        category: category(),
        description: description() || undefined,
      });
      setSuccess(true);
      // Auto-close after success
      setTimeout(() => props.onClose(), 1500);
    } catch (err) {
      setError(errorText(err, "moderation.report.failed"));
      setIsSubmitting(false);
    }
  };

  return (
    <Modal
      onClose={() => props.onClose()}
      labelledBy={titleId}
      class="relative rounded-xl border border-white/10 w-[440px] max-w-[calc(100vw-2rem)] max-h-[calc(100dvh-2rem)] overflow-y-auto shadow-2xl animate-[fadeIn_0.15s_ease-out]"
      style={{ "background-color": "var(--color-surface-layer1)" }}
      backdropClass="fixed inset-0 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      testId="report-dialog"
    >
      {/* Header */}
      <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
        <div class="flex items-center gap-3">
          <div class="w-9 h-9 rounded-lg bg-status-error/20 flex items-center justify-center">
            <Flag class="w-5 h-5 text-status-error" aria-hidden="true" />
          </div>
          <div>
            <h2 id={titleId} class="text-lg font-bold text-text-primary">
              {targetType() === "message"
                ? t("moderation.report.titleMessage")
                : t("moderation.report.titleUser")}
            </h2>
            <p class="text-xs text-text-secondary">{props.target.username}</p>
          </div>
        </div>
        <button
          type="button"
          onClick={() => props.onClose()}
          aria-label={t("common.close")}
          class="p-1.5 min-w-11 min-h-11 flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
        >
          <X class="w-5 h-5" aria-hidden="true" />
        </button>
      </div>

      {/* Content */}
      <div class="p-5 space-y-4">
        <Show
          when={!success()}
          fallback={
            <div
              role="status"
              class="p-4 rounded-lg bg-status-success/20 border border-status-success/30 text-text-primary text-sm text-center"
            >
              {t("moderation.report.sent")}
            </div>
          }
        >
          {/* Category */}
          <div role="radiogroup" aria-labelledby={reasonId} class="space-y-2">
            <p id={reasonId} class="text-sm font-medium text-text-secondary">
              {t("moderation.report.reason")}
            </p>
            <div class="space-y-1">
              <For each={CATEGORIES}>
                {(cat) => (
                  <label
                    class="flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-colors hover:bg-white/5"
                    classList={{
                      "bg-white/10 border border-white/20": category() === cat.value,
                      "border border-transparent": category() !== cat.value,
                    }}
                  >
                    <input
                      type="radio"
                      name="report-category"
                      value={cat.value}
                      checked={category() === cat.value}
                      onChange={() => setCategory(cat.value)}
                      class="accent-accent-primary"
                    />
                    <span class="text-sm text-text-primary">{t(cat.label)}</span>
                  </label>
                )}
              </For>
            </div>
          </div>

          {/* Description */}
          <div class="space-y-2">
            <label for={detailsId} class="text-sm font-medium text-text-secondary">
              {t("moderation.report.details")}
            </label>
            <textarea
              id={detailsId}
              value={description()}
              onInput={(e) => setDescription(e.currentTarget.value.slice(0, 500))}
              placeholder={t("moderation.report.detailsPlaceholder")}
              class="input-field resize-none text-sm"
              rows={3}
              maxLength={500}
            />
            <div class="text-xs text-text-secondary text-right">
              {t("moderation.report.count", { count: description().length })}
            </div>
          </div>

          <Show when={error()}>
            <div
              role="alert"
              class="p-3 rounded-lg bg-status-error/10 border border-status-error/30 text-text-primary text-sm"
            >
              {error()}
            </div>
          </Show>

          <div class="flex gap-3 justify-end">
            <Button variant="secondary" onClick={() => props.onClose()}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              onClick={handleSubmit}
              disabled={isSubmitting()}
              data-testid="report-submit"
            >
              {isSubmitting() ? t("moderation.report.submitting") : t("moderation.report.submit")}
            </Button>
          </div>
        </Show>
      </div>
    </Modal>
  );
};

export default ReportModal;
