/**
 * KeyboardShortcutsDialog - Keyboard Shortcuts Help Overlay
 *
 * Displays all available keyboard shortcuts organized by category.
 * Triggered via Ctrl+/, ? key, or the /? slash command.
 *
 * Features:
 * - Full-screen semi-transparent overlay with centered modal
 * - Shortcut categories: General, Voice, Chat
 * - Styled <kbd> key badges
 * - A modal dialog (A11Y-1): Escape or a backdrop click closes it
 */

import { Component, For, createUniqueId } from "solid-js";
import { X } from "lucide-solid";
import { appSettings } from "@/stores/settings";
import { keyCodeToLabel } from "@/lib/pttManager";
import { t, type MessageKey } from "@/i18n";
import Modal from "./Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

interface ShortcutEntry {
  keys: string[];
  description: MessageKey;
}

interface ShortcutCategory {
  title: MessageKey;
  shortcuts: ShortcutEntry[];
}

function getShortcutCategories(): ShortcutCategory[] {
  const settings = appSettings();
  const ctrl = t("common.key.ctrl");
  const shift = t("common.key.shift");
  const enter = t("common.key.enter");
  const voiceShortcuts: ShortcutEntry[] = [
    { keys: [ctrl, shift, "M"], description: "common.shortcuts.mute" },
    { keys: [ctrl, shift, "D"], description: "common.shortcuts.deafen" },
  ];

  if (settings?.voice.push_to_talk && settings.voice.push_to_talk_key) {
    voiceShortcuts.push({
      keys: [keyCodeToLabel(settings.voice.push_to_talk_key)],
      description: "common.shortcuts.pushToTalk",
    });
  }

  if (settings?.voice.push_to_mute && settings.voice.push_to_mute_key) {
    voiceShortcuts.push({
      keys: [keyCodeToLabel(settings.voice.push_to_mute_key)],
      description: "common.shortcuts.pushToMute",
    });
  }

  return [
    {
      title: "common.shortcuts.general",
      shortcuts: [
        { keys: [ctrl, "K"], description: "common.shortcuts.palette" },
        { keys: [ctrl, shift, "F"], description: "common.shortcuts.globalSearch" },
        { keys: [ctrl, "/"], description: "common.shortcuts.toggleDialog" },
      ],
    },
    { title: "common.shortcuts.voice", shortcuts: voiceShortcuts },
    {
      title: "common.shortcuts.chat",
      shortcuts: [
        { keys: [ctrl, "F"], description: "common.shortcuts.channelSearch" },
        { keys: [enter], description: "common.shortcuts.send" },
        { keys: [shift, enter], description: "common.shortcuts.newLine" },
        { keys: [ctrl, "B"], description: "common.shortcuts.bold" },
        { keys: [ctrl, "I"], description: "common.shortcuts.italic" },
        { keys: [ctrl, "E"], description: "common.shortcuts.code" },
      ],
    },
  ];
}

interface KeyboardShortcutsDialogProps {
  onClose: () => void;
}

const KeyboardShortcutsDialog: Component<KeyboardShortcutsDialogProps> = (props) => {
  const titleId = createUniqueId();

  return (
    <Modal
      onClose={() => props.onClose()}
      labelledBy={titleId}
      layer={MODAL_LAYER.dialog + 30}
      backdropClass="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4"
      class="w-[560px] max-w-full max-h-[80vh] border border-white/10 shadow-2xl rounded-xl overflow-hidden flex flex-col"
      style={{ "background-color": "var(--color-surface-layer2)" }}
      testId="shortcuts-dialog"
    >
      {/* Header */}
      <div class="px-6 py-4 border-b border-white/5 flex items-center justify-between">
        <h2 id={titleId} class="text-lg font-semibold text-text-primary">
          {t("common.shortcuts.title")}
        </h2>
        <button
          type="button"
          onClick={() => props.onClose()}
          class="p-1.5 tap-target rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
          title={t("common.close")}
          aria-label={t("common.close")}
        >
          <X class="w-5 h-5" aria-hidden="true" />
        </button>
      </div>

      {/* Content */}
      <div class="flex-1 overflow-y-auto px-6 py-4 space-y-6">
        <For each={getShortcutCategories()}>
          {(category) => (
            <section>
              <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wider mb-3">
                {t(category.title)}
              </h3>
              <dl class="space-y-2">
                <For each={category.shortcuts}>
                  {(shortcut) => (
                    <div class="flex items-center justify-between py-1.5">
                      <dt class="text-sm text-text-primary">{t(shortcut.description)}</dt>
                      <dd class="flex items-center gap-1">
                        <For each={shortcut.keys}>
                          {(key, index) => (
                            <>
                              {index() > 0 && (
                                <span class="text-xs text-text-secondary mx-0.5">+</span>
                              )}
                              <kbd class="inline-flex items-center justify-center min-w-[24px] h-6 px-1.5 text-xs font-medium text-text-primary bg-surface-base rounded border border-white/10 shadow-sm">
                                {key}
                              </kbd>
                            </>
                          )}
                        </For>
                      </dd>
                    </div>
                  )}
                </For>
              </dl>
            </section>
          )}
        </For>
      </div>

      {/* Footer */}
      <div class="px-6 py-3 border-t border-white/5 bg-surface-base">
        <p class="text-center text-xs text-text-secondary">{t("common.shortcuts.footer")}</p>
      </div>
    </Modal>
  );
};

export default KeyboardShortcutsDialog;
