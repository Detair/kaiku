/**
 * CustomStatusModal - Modal for setting custom user status
 *
 * Allows users to set a custom status message with optional emoji
 * and expiry time.
 */

import { Component, createSignal, For } from "solid-js";
import type { CustomStatus } from "@/lib/types";

interface CustomStatusModalProps {
  currentStatus?: CustomStatus | null;
  onSave: (status: CustomStatus | null) => void;
  onClose: () => void;
}

const expiryOptions = [
  { value: null, label: "Don't clear" },
  { value: 30, label: "30 minutes" },
  { value: 60, label: "1 hour" },
  { value: 240, label: "4 hours" },
  { value: 1440, label: "1 day" },
];

const CustomStatusModal: Component<CustomStatusModalProps> = (props) => {
  const [text, setText] = createSignal(props.currentStatus?.text ?? "");
  const [emoji, setEmoji] = createSignal(props.currentStatus?.emoji ?? "");
  const [expiryMinutes, setExpiryMinutes] = createSignal<number | null>(null);

  const handleSave = () => {
    if (!text().trim()) {
      props.onSave(null);
    } else {
      const expiresAt = expiryMinutes()
        ? new Date(Date.now() + expiryMinutes()! * 60 * 1000).toISOString()
        : undefined;
      props.onSave({
        text: text().trim(),
        emoji: emoji() || undefined,
        expiresAt,
      });
    }
    props.onClose();
  };

  const handleClear = () => {
    props.onSave(null);
    props.onClose();
  };

  return (
    <div
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      onClick={props.onClose}
    >
      <div
        class="bg-surface-layer2 rounded-xl p-4 w-96 shadow-xl border border-white/10"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 class="text-lg font-semibold text-text-primary mb-4">
          Set Custom Status
        </h3>

        <div class="flex gap-2 mb-4">
          <input
            type="text"
            placeholder="😀"
            value={emoji()}
            onInput={(e) => setEmoji(e.currentTarget.value)}
            class="w-12 px-2 py-2 bg-surface-base rounded-lg text-center text-xl border border-white/10 focus:border-accent-primary focus:outline-none transition-colors"
            maxLength={2}
          />
          <input
            type="text"
            placeholder="What's happening?"
            value={text()}
            onInput={(e) => setText(e.currentTarget.value)}
            class="flex-1 px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors"
            maxLength={128}
          />
        </div>

        <div class="mb-4">
          <label class="text-sm text-text-secondary mb-1 block">
            Clear after
          </label>
          <select
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
                <option value={value ?? ""}>{label}</option>
              )}
            </For>
          </select>
        </div>

        <div class="flex justify-between">
          <button
            onClick={handleClear}
            class="px-4 py-2 text-text-secondary hover:text-text-primary transition-colors"
          >
            Clear Status
          </button>
          <div class="flex gap-2">
            <button
              onClick={props.onClose}
              class="px-4 py-2 bg-surface-base rounded-lg hover:bg-white/10 transition-colors text-text-primary"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              class="px-4 py-2 bg-accent-primary rounded-lg text-white hover:bg-accent-primary/90 transition-colors"
            >
              Save
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CustomStatusModal;
