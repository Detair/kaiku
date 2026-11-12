/**
 * GlobalSearchDialog — message search over all servers and conversations
 * (Ctrl+Shift+F, Ctrl+F). A modal dialog (A11Y-1): focus goes to the search
 * field, Escape or a click beside it closes it, focus returns.
 */

import { Component } from "solid-js";
import Modal from "@/components/ui/Modal";
import SearchPanel from "./SearchPanel";
import { t } from "@/i18n";

interface GlobalSearchDialogProps {
  initialScope: "channel" | "all";
  channelId?: string;
  onClose: () => void;
}

const GlobalSearchDialog: Component<GlobalSearchDialogProps> = (props) => (
  <Modal
    onClose={() => props.onClose()}
    label={t("chat.search.dialogLabel")}
    backdropClass="fixed inset-0 flex items-start justify-center pt-[10vh] px-4 bg-black/50"
    class="relative w-[640px] max-w-full h-[70vh] rounded-xl border border-white/10 shadow-2xl overflow-hidden"
    style={{ "background-color": "var(--color-surface-layer2)" }}
    testId="global-search-dialog"
  >
    <SearchPanel
      mode="global"
      initialScope={props.initialScope}
      channelId={props.channelId}
      onClose={() => props.onClose()}
    />
  </Modal>
);

export default GlobalSearchDialog;
