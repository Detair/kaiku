/**
 * Block Confirm Modal
 *
 * Asks before blocking a person. A modal dialog (A11Y-1): focus starts on
 * "Cancel", Tab stays inside, Escape closes, focus returns to the opener.
 */

import { Component, createSignal, createUniqueId, Show } from "solid-js";
import { X, Ban } from "lucide-solid";
import { blockUser } from "@/stores/friends";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import Modal from "@/components/ui/Modal";
import Button from "@/components/ui/Button";

interface BlockConfirmModalProps {
  userId: string;
  username: string;
  displayName?: string;
  onClose: () => void;
}

const BlockConfirmModal: Component<BlockConfirmModalProps> = (props) => {
  const titleId = createUniqueId();
  const bodyId = createUniqueId();
  let cancelRef: HTMLButtonElement | undefined;
  const [isBlocking, setIsBlocking] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const name = () => props.displayName || props.username;

  const handleBlock = async () => {
    setIsBlocking(true);
    setError(null);

    try {
      await blockUser(props.userId);
      props.onClose();
    } catch (err) {
      setError(errorText(err, "moderation.block.failed"));
      setIsBlocking(false);
    }
  };

  return (
    <Modal
      onClose={() => props.onClose()}
      labelledBy={titleId}
      describedBy={bodyId}
      initialFocus={() => cancelRef}
      class="relative rounded-xl border border-white/10 w-[400px] max-w-[calc(100vw-2rem)] shadow-2xl animate-[fadeIn_0.15s_ease-out]"
      style={{ "background-color": "var(--color-surface-layer1)" }}
      backdropClass="fixed inset-0 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      testId="block-dialog"
    >
      {/* Header */}
      <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
        <div class="flex items-center gap-3">
          <div class="w-9 h-9 rounded-lg bg-status-error/20 flex items-center justify-center">
            <Ban class="w-5 h-5 text-status-error" aria-hidden="true" />
          </div>
          <h2 id={titleId} class="text-lg font-bold text-text-primary">
            {t("moderation.block.title")}
          </h2>
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
        <div id={bodyId} class="space-y-2">
          <p class="text-text-primary text-sm font-medium">
            {t("moderation.block.question", { name: name() })}
          </p>
          <p class="text-text-secondary text-sm">{t("moderation.block.effect")}</p>
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
          <Button ref={cancelRef} variant="secondary" onClick={() => props.onClose()}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="danger"
            onClick={handleBlock}
            disabled={isBlocking()}
            data-testid="block-confirm"
          >
            {isBlocking() ? t("moderation.block.blocking") : t("moderation.block.confirm")}
          </Button>
        </div>
      </div>
    </Modal>
  );
};

export default BlockConfirmModal;
