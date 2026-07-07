/**
 * MessageComponents — render bot-authored interactive components (buttons and
 * select menus). Clicking a non-link button or choosing a select option sends a
 * `component_interaction` frame over the WebSocket; the server routes it to the
 * owning bot, which replies via the existing interaction path (a channel message
 * or an ephemeral response). Link buttons are plain anchors and need no round-trip.
 */

import { Component, For, Show } from "solid-js";
import { wsSend } from "@/lib/tauri";
import type { MessageActionRow, MessageComponent } from "@/lib/types";

const BUTTON_STYLES: Record<string, string> = {
  primary: "bg-accent-primary text-on-accent hover:bg-accent-primary/90",
  secondary:
    "bg-[var(--color-surface-layer2)] text-text-primary hover:bg-white/10",
  success: "bg-accent-success text-on-accent hover:bg-accent-success/90",
  danger: "bg-accent-danger text-on-accent hover:bg-accent-danger/90",
  link: "bg-[var(--color-surface-layer2)] text-accent-primary hover:underline",
};

const MessageComponents: Component<{
  messageId: string;
  rows: MessageActionRow[];
}> = (props) => {
  const dispatch = (customId: string, values: string[] = []) => {
    void wsSend({
      type: "component_interaction",
      message_id: props.messageId,
      custom_id: customId,
      values,
    });
  };

  const renderButton = (c: Extract<MessageComponent, { type: "button" }>) => {
    const cls = `px-3 py-1.5 rounded-md text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
      BUTTON_STYLES[c.style] ?? BUTTON_STYLES.secondary
    }`;
    if (c.style === "link" && c.url) {
      return (
        <a
          href={c.url}
          target="_blank"
          rel="noopener noreferrer nofollow"
          class={`inline-block ${cls}`}
          classList={{ "pointer-events-none opacity-50": c.disabled }}
        >
          {c.label ?? "Open"}
        </a>
      );
    }
    return (
      <button
        type="button"
        class={cls}
        disabled={c.disabled || !c.custom_id}
        onClick={() => c.custom_id && dispatch(c.custom_id)}
      >
        {c.label ?? "Button"}
      </button>
    );
  };

  const renderSelect = (
    c: Extract<MessageComponent, { type: "select_menu" }>,
  ) => (
    <select
      class="px-3 py-1.5 rounded-md text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none disabled:opacity-50"
      disabled={c.disabled}
      onChange={(e) => dispatch(c.custom_id, [e.currentTarget.value])}
    >
      <Show when={c.placeholder}>
        <option value="" disabled selected>
          {c.placeholder}
        </option>
      </Show>
      <For each={c.options}>
        {(o) => (
          <option value={o.value} selected={o.default}>
            {o.label}
          </option>
        )}
      </For>
    </select>
  );

  return (
    <div class="flex flex-col gap-2 mt-2">
      <For each={props.rows}>
        {(row) => (
          <div class="flex flex-wrap items-center gap-2">
            <For each={row.components}>
              {(c) => (c.type === "button" ? renderButton(c) : renderSelect(c))}
            </For>
          </div>
        )}
      </For>
    </div>
  );
};

export default MessageComponents;
