/**
 * Add Friend Modal
 *
 * Modal for sending friend requests by username.
 */

import { Component, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { sendFriendRequest } from "@/stores/friends";

interface AddFriendProps {
  onClose: () => void;
}

const AddFriend: Component<AddFriendProps> = (props) => {
  const [username, setUsername] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [success, setSuccess] = createSignal(false);
  const [isSubmitting, setIsSubmitting] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    setSuccess(false);

    const usernameValue = username().trim();
    if (!usernameValue) {
      setError("Please enter a username");
      return;
    }

    setIsSubmitting(true);

    try {
      await sendFriendRequest(usernameValue);
      setSuccess(true);
      setUsername("");
      // Close modal after 1.5 seconds
      setTimeout(() => {
        props.onClose();
      }, 1500);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to send friend request");
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
          class="bg-surface-base border border-white/10 rounded-2xl w-full max-w-md p-6"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between mb-6">
            <h2 class="text-xl font-bold text-text-primary">Add Friend</h2>
            <button
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
            >
              <svg
                class="w-6 h-6"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            </button>
          </div>

          {/* Description */}
          <p class="text-text-secondary text-sm mb-4">
            Enter the username of the person you want to add as a friend.
          </p>

          {/* Form */}
          <form onSubmit={handleSubmit} class="space-y-4">
            {/* Username Input */}
            <div>
              <label
                for="username"
                class="block text-sm font-medium text-text-primary mb-2"
              >
                Username
              </label>
              <input
                id="username"
                type="text"
                value={username()}
                onInput={(e) => setUsername(e.currentTarget.value)}
                placeholder="Enter username..."
                class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-lg text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary transition-colors"
                disabled={isSubmitting()}
                autocomplete="off"
              />
            </div>

            {/* Error Message */}
            {error() && (
              <div class="p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-sm">
                {error()}
              </div>
            )}

            {/* Success Message */}
            {success() && (
              <div class="p-3 bg-green-500/10 border border-green-500/20 rounded-lg text-green-400 text-sm">
                Friend request sent successfully!
              </div>
            )}

            {/* Actions */}
            <div class="flex gap-3 justify-end pt-2">
              <button
                type="button"
                onClick={props.onClose}
                class="px-4 py-2 text-text-secondary hover:text-text-primary transition-colors"
                disabled={isSubmitting()}
              >
                Cancel
              </button>
              <button
                type="submit"
                class="px-4 py-2 bg-accent-primary text-surface-base rounded-lg font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
                disabled={isSubmitting()}
              >
                {isSubmitting() ? "Sending..." : "Send Request"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Portal>
  );
};

export default AddFriend;
