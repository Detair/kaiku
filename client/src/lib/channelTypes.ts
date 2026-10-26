import type { ChannelType } from "./types";

/**
 * Channel types that carry readable messages and therefore unread state.
 * Forum and announcement channels count like text channels; voice does not.
 */
export function isTextLikeChannelType(type: ChannelType): boolean {
  return type === "text" || type === "forum" || type === "announcement";
}
