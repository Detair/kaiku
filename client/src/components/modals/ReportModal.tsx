/**
 * Report Modal
 *
 * Allows users to report a user or message to admins.
 */

import { Component, createSignal, For } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Flag } from "lucide-solid";
import * as tauri from "@/lib/tauri";

export interface ReportTarget {
  userId: string;
  username: string;
  messageId?: string;
}

interface ReportModalProps {
  target: ReportTarget;
  onClose: () => void;
}

const CATEGORIES: { value: tauri.CreateReportRequest["category"]; label: string }[] = [
  { value: "harassment", label: "Harassment" },
  { value: "spam", label: "Spam" },
  { value: "inappropriate_content", label: "Inappropriate Content" },
  { value: "impersonation", label: "Impersonation" },
  { value: "other", label: "Other" },
];

const ReportModal: Component<ReportModalProps> = (props) => {
  const [category, setCategory] = createSignal<tauri.CreateReportRequest["category"]>("harassment");
  const [description, setDescription] = createSignal("");
  const [isSubmitting, setIsSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [success, setSuccess] = createSignal(false);

  const targetType = () => (props.target.messageId ? "message" : "user") as "user" | "message";

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
      setError(err instanceof Error ? err.message : "Failed to submit report");
      setIsSubmitting(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) props.onClose();
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center"
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div
          class="absolute inset-0 bg-black/60 backdrop-blur-sm"
          onClick={handleBackdropClick}
        />

        <div
          class="relative rounded-xl border border-white/10 w-[440px] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
          style="background-color: var(--color-surface-layer1)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-9 h-9 rounded-lg bg-status-error/20 flex items-center justify-center">
                <Flag class="w-5 h-5 text-status-error" />
              </div>
              <div>
                <h2 class="text-lg font-bold text-text-primary">
                  Report {targetType() === "message" ? "Message" : "User"}
                </h2>
                <p class="text-xs text-text-secondary">
                  {props.target.username}
                </p>
              </div>
            </div>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Content */}
          <div class="p-5 space-y-4">
            {success() ? (
              <div class="p-4 rounded-lg bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-sm text-center">
                Report submitted. Thank you for helping keep the community safe.
              </div>
            ) : (
              <>
                {/* Category */}
                <div class="space-y-2">
                  <label class="text-sm font-medium text-text-secondary">Reason</label>
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
                            name="category"
                            value={cat.value}
                            checked={category() === cat.value}
                            onChange={() => setCategory(cat.value)}
                            class="accent-accent-primary"
                          />
                          <span class="text-sm text-text-primary">{cat.label}</span>
                        </label>
                      )}
                    </For>
                  </div>
                </div>

                {/* Description */}
                <div class="space-y-2">
                  <label class="text-sm font-medium text-text-secondary">
                    Details <span class="text-text-secondary/50">(optional, max 500 chars)</span>
                  </label>
                  <textarea
                    value={description()}
                    onInput={(e) => setDescription(e.currentTarget.value.slice(0, 500))}
                    placeholder="Provide additional context..."
                    class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary placeholder-text-secondary/50 focus:outline-none focus:border-accent-primary resize-none text-sm"
                    rows={3}
                    maxLength={500}
                  />
                  <div class="text-xs text-text-secondary/50 text-right">
                    {description().length}/500
                  </div>
                </div>

                {error() && (
                  <div class="p-3 rounded-lg bg-status-error/10 border border-status-error/30 text-status-error text-sm">
                    {error()}
                  </div>
                )}

                <div class="flex gap-3 justify-end">
                  <button
                    onClick={props.onClose}
                    class="px-4 py-2 rounded-lg bg-white/10 text-text-primary font-medium transition-colors hover:bg-white/20"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleSubmit}
                    disabled={isSubmitting()}
                    class="px-4 py-2 rounded-lg bg-status-error text-white font-medium transition-colors hover:bg-status-error/90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    {isSubmitting() ? "Submitting..." : "Submit Report"}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default ReportModal;
