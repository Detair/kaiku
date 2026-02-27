import type { TokenizerAndRendererExtension } from "marked";

/** Token produced by the spoiler tokenizer for ||text|| syntax. */
export interface SpoilerToken {
  type: "spoiler";
  raw: string;
  text: string;
}

/** Custom marked extension for ||spoiler|| syntax. */
export const spoilerExtension: TokenizerAndRendererExtension = {
  name: "spoiler",
  level: "inline" as const,
  start(src: string) {
    const index = src.indexOf("||");
    return index >= 0 ? index : undefined;
  },
  tokenizer(src: string) {
    // Limit spoiler content to 500 chars to prevent ReDoS
    const match = /^\|\|(.{1,500}?)\|\|/.exec(src);
    if (match) {
      return {
        type: "spoiler",
        raw: match[0],
        text: match[1],
      };
    }
    return undefined;
  },
  renderer(token) {
    const spoilerToken = token as unknown as SpoilerToken;
    return `<span class="spoiler" data-spoiler="true">${spoilerToken.text}</span>`;
  },
};
