/**
 * JoinGuildModal - Modal for joining a guild via invite code or URL
 *
 * Accepts both bare invite codes (aBcD1234) and full invite URLs
 * (https://example.com/invite/aBcD1234).
 */

import { Component, createSignal, Show } from "solid-js";
import { X } from "lucide-solid";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { createDialog } from "@/lib/a11y/dialog";
import { joinViaInviteCode } from "@/stores/guilds";
import { Portal } from "solid-js/web";
import { extractInviteCode } from "@/lib/invites";

interface JoinGuildModalProps {
  onClose: () => void;
}

const JoinGuildModal: Component<JoinGuildModalProps> = (props) => {
  let inputEl: HTMLInputElement | undefined;
  const dialog = createDialog({
    onClose: () => props.onClose(),
    initialFocus: () => inputEl,
  });

  const [input, setInput] = createSignal("");
  const [isJoining, setIsJoining] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const inviteCode = () => extractInviteCode(input());

  const handleSubmit = async (e: Event) => {
    e.preventDefault();

    const code = inviteCode();
    if (!code) {
      setError(t("guilds.join.invalid"));
      return;
    }

    setIsJoining(true);
    setError(null);

    try {
      await joinViaInviteCode(code);
      props.onClose();
    } catch (err) {
      console.error("Failed to join guild via invite code:", code, err);
      setError(errorText(err, "guilds.join.failed"));
    } finally {
      setIsJoining(false);
    }
  };

  return (
    <Portal>
      {/* Backdrop click closes, like the other dialogs (NAV-11) */}
      <div
        class="fixed inset-0 bg-black/60 flex items-center justify-center z-50"
        onClick={() => props.onClose()}
      >
        <div
          ref={dialog.ref}
          {...dialog.dialogProps}
          onClick={(e) => e.stopPropagation()}
          class="outline-none border border-white/10 rounded-2xl w-[480px] max-w-[calc(100vw-2rem)] max-h-[min(600px,calc(100dvh-2rem))] flex flex-col shadow-2xl"
          style={{"background-color":"var(--color-surface-base)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 id={dialog.titleId} class="text-xl font-bold text-text-primary">{t("guilds.join.title")}</h2>
            <button
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
              aria-label={t("common.close")}
            >
              <X size={24} aria-hidden="true" />
            </button>
          </div>

          {/* Content */}
          <form onSubmit={handleSubmit} class="flex-1 overflow-y-auto p-6">
            <div class="space-y-4">
              {/* Invite Input */}
              <div>
                <label for="join-guild-invite" class="block text-sm font-semibold text-text-primary mb-2">
                  {t("guilds.join.label")}
                </label>
                <input
                  ref={inputEl}
                  id="join-guild-invite"
                  aria-describedby="join-guild-hint"
                  type="text"
                  value={input()}
                  onInput={(e) => {
                    setInput(e.currentTarget.value);
                    setError(null);
                  }}
                  placeholder={t("guilds.join.placeholder")}
                  class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                  style={{"background-color":"var(--color-surface-layer2)"}}
                  disabled={isJoining()}
                />
                <p id="join-guild-hint" class="text-xs text-text-secondary mt-1">
                  {t("guilds.join.hint")}
                </p>
              </div>

              {/* Error Message */}
              <Show when={error()}>
                <div
                  role="alert"
                  class="p-3 rounded-lg"
                  style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)"}}
                >
                  <p class="text-sm" style={{"color":"var(--color-error-text)"}}>
                    {error()}
                  </p>
                </div>
              </Show>
            </div>
          </form>

          {/* Footer */}
          <div class="flex items-center justify-end gap-3 p-6 border-t border-white/10">
            <button
              type="button"
              onClick={props.onClose}
              class="px-4 py-2 text-text-primary hover:bg-surface-layer2 rounded-lg transition-colors state-disabled-quiet"
              disabled={isJoining()}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              class="px-6 py-2 bg-accent-primary hover:bg-accent-primary/90 text-on-accent font-semibold rounded-lg transition-colors state-disabled"
              disabled={isJoining() || !inviteCode()}
            >
              {isJoining() ? t("guilds.join.joining") : t("guilds.join.submit")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default JoinGuildModal;
