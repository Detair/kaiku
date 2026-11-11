/**
 * Pure helpers for the message list: day separators, thread availability and
 * the short screen reader announcement for new messages.
 */

import { getLocale, plural, t } from "@/i18n";
import type { Message } from "@/lib/types";

/** Local calendar day of an ISO timestamp, as `YYYY-M-D`. */
export function dayKey(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/** "Today", "Yesterday" or a full date such as "Monday, 10 November 2026". */
export function dayLabel(iso: string, now: Date = new Date()): string {
  const day = new Date(iso);
  const key = dayKey(iso);
  if (key === dayKey(now.toISOString())) return t("chat.list.day.today");
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (key === dayKey(yesterday.toISOString())) return t("chat.list.day.yesterday");
  return new Intl.DateTimeFormat(getLocale(), {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: day.getFullYear() === now.getFullYear() ? undefined : "numeric",
  }).format(day);
}

/**
 * Whether messages in this channel may open a thread. Direct messages have no
 * thread panel, so they never offer one; server channels follow the server's
 * setting.
 */
export function threadsEnabledFor(
  isDirectMessage: boolean,
  guildId: string | undefined,
  areThreadsEnabled: (guildId: string | undefined) => boolean,
): boolean {
  if (isDirectMessage) return false;
  return areThreadsEnabled(guildId);
}

const PREVIEW_LENGTH = 120;

/**
 * What a screen reader hears for messages that just arrived from others:
 * "Oma: Hallo …" for one, "3 new messages" for several. `null` for none.
 */
export function announcementFor(messages: Message[]): string | null {
  if (messages.length === 0) return null;
  if (messages.length > 1) {
    return plural(messages.length, {
      one: "chat.list.newCount.one",
      other: "chat.list.newCount.other",
    });
  }
  const message = messages[0];
  const name = message.author.display_name || message.author.username;
  const text = (message.content ?? "").replace(/\s+/g, " ").trim();
  if (!text) return t("chat.list.announce.file", { name });
  const preview = [...text].length > PREVIEW_LENGTH
    ? `${[...text].slice(0, PREVIEW_LENGTH).join("")}…`
    : text;
  return t("chat.list.announce.message", { name, text: preview });
}
