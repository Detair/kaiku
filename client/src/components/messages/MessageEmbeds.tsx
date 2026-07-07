/**
 * MessageEmbeds — render bot-authored rich embed cards.
 *
 * Embed text is attacker-influenced (bots are semi-trusted), so titles/footers
 * render as plain text and description/field-values go through markdown + an
 * ISOLATED DOMPurify instance (per the #631 global-hook gotcha — never share a
 * purifier across renderers).
 */

import { Component, For, Show } from "solid-js";
import { renderEmbedRich, embedColorHex } from "@/lib/embedSanitize";
import type { MessageEmbed } from "@/lib/types";

const renderRich = renderEmbedRich;
const colorHex = embedColorHex;

const MessageEmbeds: Component<{ embeds: MessageEmbed[] }> = (props) => {
  return (
    <div class="flex flex-col gap-2 mt-1">
      <For each={props.embeds}>
        {(e) => (
          <div
            class="rounded-md p-3 text-sm max-w-[520px]"
            style={{
              "background-color": "var(--color-surface-layer1)",
              "border-left": `4px solid ${colorHex(e.color)}`,
            }}
          >
            <Show when={e.author}>
              <div class="text-text-secondary text-xs mb-1">{e.author!.name}</div>
            </Show>
            <Show when={e.title}>
              <div class="font-semibold text-text-primary">
                <Show when={e.url} fallback={<span>{e.title}</span>}>
                  <a
                    href={e.url}
                    target="_blank"
                    rel="noopener noreferrer nofollow"
                    class="text-accent-primary hover:underline"
                  >
                    {e.title}
                  </a>
                </Show>
              </div>
            </Show>
            <Show when={e.description}>
              {/* eslint-disable-next-line solid/no-innerhtml */}
              <div
                class="text-text-primary mt-1 break-words"
                innerHTML={renderRich(e.description!)}
              />
            </Show>
            <Show when={e.fields && e.fields.length > 0}>
              <div class="grid grid-cols-2 gap-2 mt-2">
                <For each={e.fields}>
                  {(f) => (
                    <div classList={{ "col-span-2": !f.inline }}>
                      <div class="text-text-primary font-medium text-xs">
                        {f.name}
                      </div>
                      {/* eslint-disable-next-line solid/no-innerhtml */}
                      <div
                        class="text-text-secondary text-xs break-words"
                        innerHTML={renderRich(f.value)}
                      />
                    </div>
                  )}
                </For>
              </div>
            </Show>
            <Show when={e.image}>
              <img
                src={e.image}
                alt=""
                class="mt-2 rounded max-w-full"
                loading="lazy"
              />
            </Show>
            <Show when={e.footer}>
              <div class="text-text-muted text-xs mt-2">{e.footer!.text}</div>
            </Show>
          </div>
        )}
      </For>
    </div>
  );
};

export default MessageEmbeds;
