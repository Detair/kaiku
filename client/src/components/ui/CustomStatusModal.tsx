/**
 * CustomStatusModal - Modal for setting custom user status
 *
 * Allows users to set a custom status message with optional emoji
 * and expiry time.
 */

import { Component, createSignal, createUniqueId, For } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import type { CustomStatus } from "@/lib/types";
import Modal from "./Modal";
import Button from "./Button";

// A pictograph, not text: nothing to translate, so it stays out of the catalog.
const EMOJI_PLACEHOLDER = "😀";

interface CustomStatusModalProps {
  currentStatus?: CustomStatus | null;
  onSave: (status: CustomStatus | null) => void;
  onClose: () => void;
}

const expiryOptions: { value: number | null; label: MessageKey }[] = [
  { value: null, label: "home.customStatus.dontClear" },
  { value: 30, label: "home.customStatus.thirtyMinutes" },
  { value: 60, label: "home.customStatus.oneHour" },
  { value: 240, label: "home.customStatus.fourHours" },
  { value: 1440, label: "home.customStatus.oneDay" },
];

const CustomStatusModal: Component<CustomStatusModalProps> = (props) => {
  const titleId = createUniqueId();
  let textRef: HTMLInputElement | undefined;
  const expiryId = createUniqueId();

  const [text, setText] = createSignal(props.currentStatus?.text ?? "");
  const [emoji, setEmoji] = createSignal(props.currentStatus?.emoji ?? "");
  const [expiryMinutes, setExpiryMinutes] = createSignal<number | null>(null);

  const handleSave = () => {
    if (!text().trim()) {
      props.onSave(null);
    } else {
      const expires_at = expiryMinutes()
        ? new Date(Date.now() + expiryMinutes()! * 60 * 1000).toISOString()
        : undefined;
      props.onSave({
        text: text().trim(),
        emoji: emoji() || undefined,
        expires_at,
      });
    }
    props.onClose();
  };

  const handleClear = () => {
    props.onSave(null);
    props.onClose();
  };

  return (
    <Modal
      onClose={() => props.onClose()}
      labelledBy={titleId}
      initialFocus={() => textRef}
      backdropClass="fixed inset-0 bg-black/50 flex items-center justify-center p-4"
      class="bg-surface-layer2 rounded-xl p-4 w-96 max-w-full shadow-xl border border-white/10"
      testId="custom-status-dialog"
    >
        <h3 id={titleId} class="text-lg font-semibold text-text-primary mb-4">
          {t("home.customStatus.title")}
        </h3>

        <div class="flex gap-2 mb-4">
          <input
            type="text"
            placeholder={EMOJI_PLACEHOLDER}
            aria-label={t("home.customStatus.emojiLabel")}
            value={emoji()}
            onInput={(e) => setEmoji(e.currentTarget.value)}
            class="w-12 px-2 py-2 bg-surface-base rounded-lg text-center text-xl border border-white/10 focus:border-accent-primary focus:outline-none transition-colors"
            maxLength={2}
          />
          <input
            type="text"
            ref={textRef}
            placeholder={t("home.customStatus.placeholder")}
            aria-label={t("home.customStatus.textLabel")}
            value={text()}
            onInput={(e) => setText(e.currentTarget.value)}
            class="flex-1 px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors"
            maxLength={128}
          />
        </div>

        <div class="mb-4">
          <label for={expiryId} class="text-sm text-text-secondary mb-1 block">
            {t("home.customStatus.clearAfter")}
          </label>
          <select
            id={expiryId}
            value={expiryMinutes() ?? ""}
            onChange={(e) =>
              setExpiryMinutes(
                e.currentTarget.value ? Number(e.currentTarget.value) : null,
              )
            }
            class="w-full px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors"
          >
            <For each={expiryOptions}>
              {({ value, label }) => (
                <option value={value ?? ""}>{t(label)}</option>
              )}
            </For>
          </select>
        </div>

        <div class="flex justify-between">
          <Button variant="ghost" onClick={handleClear}>
            {t("home.customStatus.clear")}
          </Button>
          <div class="flex gap-2">
            <Button variant="secondary" onClick={() => props.onClose()}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleSave}>{t("common.save")}</Button>
          </div>
        </div>
    </Modal>
  );
};

export default CustomStatusModal;
