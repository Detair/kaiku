/**
 * Add Friend Modal
 *
 * Modal for sending friend requests by username.
 */

import { Component, createSignal, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { X } from "lucide-solid";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { createDialog } from "@/lib/a11y/dialog";
import { sendFriendRequest } from "@/stores/friends";

interface AddFriendProps {
  onClose: () => void;
}

const AddFriend: Component<AddFriendProps> = (props) => {
  const [username, setUsername] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [success, setSuccess] = createSignal(false);
  const [isSubmitting, setIsSubmitting] = createSignal(false);

  let input: HTMLInputElement | undefined;
  const dialog = createDialog({
    onClose: () => props.onClose(),
    initialFocus: () => input,
  });

  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(closeTimer));

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    setSuccess(false);

    const usernameValue = username().trim();
    if (!usernameValue) {
      setError(t("home.addFriend.usernameRequired"));
      return;
    }

    setIsSubmitting(true);

    try {
      await sendFriendRequest(usernameValue);
      setSuccess(true);
      setUsername("");
      // Close modal after 1.5 seconds
      closeTimer = setTimeout(() => {
        props.onClose();
      }, 1500);
    } catch (err) {
      setError(errorText(err, "home.addFriend.failed"));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
        onClick={props.onClose}
      >
        <div
          ref={dialog.ref}
          {...dialog.dialogProps}
          class="bg-surface-base border border-white/10 rounded-2xl w-full max-w-md mx-4 p-6 outline-none"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between mb-6">
            <h2 id={dialog.titleId} class="text-xl font-bold text-text-primary">
              {t("home.sidebar.addFriend")}
            </h2>
            <button
              type="button"
              onClick={props.onClose}
              class="p-1 rounded text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
              aria-label={t("common.close")}
            >
              <X class="w-6 h-6" aria-hidden="true" />
            </button>
          </div>

          {/* Description */}
          <p class="text-text-secondary text-sm mb-4">
            {t("home.addFriend.description")}
          </p>

          {/* Form */}
          <form onSubmit={handleSubmit} class="space-y-4">
            {/* Username Input */}
            <div>
              <label
                for="add-friend-username"
                class="block text-sm font-medium text-text-primary mb-2"
              >
                {t("common.username")}
              </label>
              <input
                ref={input}
                id="add-friend-username"
                data-testid="add-friend-input"
                type="text"
                value={username()}
                onInput={(e) => setUsername(e.currentTarget.value)}
                placeholder={t("home.addFriend.placeholder")}
                class="w-full px-4 py-2 border border-white/10 rounded-lg text-text-input placeholder-text-placeholder focus:outline-none focus:border-accent-primary transition-colors"
                style={{"background-color":"var(--color-surface-layer1)"}}
                disabled={isSubmitting()}
                autocomplete="off"
              />
            </div>

            {/* Error Message */}
            {error() && (
              <div
                role="alert"
                class="p-3 rounded-lg text-sm"
                style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
              >
                {error()}
              </div>
            )}

            {/* Success Message */}
            {success() && (
              <div role="status" class="p-3 bg-accent-success/10 border border-accent-success/30 rounded-lg text-text-primary text-sm">
                {t("home.addFriend.sent")}
              </div>
            )}

            {/* Actions */}
            <div class="flex gap-3 justify-end pt-2">
              <button
                type="button"
                onClick={props.onClose}
                class="px-4 py-2 text-text-secondary hover:text-text-primary transition-colors state-disabled-quiet"
                disabled={isSubmitting()}
              >
                {t("common.cancel")}
              </button>
              <button
                type="submit"
                data-testid="add-friend-submit"
                class="px-4 py-2 bg-accent-primary text-on-accent rounded-lg font-medium hover:opacity-90 transition-opacity state-disabled"
                disabled={isSubmitting()}
              >
                {isSubmitting() ? t("home.addFriend.sending") : t("home.addFriend.send")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Portal>
  );
};

export default AddFriend;
