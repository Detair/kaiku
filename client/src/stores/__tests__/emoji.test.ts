import { describe, it, expect } from "vitest";
import {
  addEmojiToRecentsArray,
  searchEmojisInArray,
  MAX_RECENTS,
  type Emoji,
} from "../emoji";

// Test data
const createEmoji = (
  id: string,
  name: string,
  category: string = "smileys",
  keywords: string[] = [],
): Emoji => ({
  id,
  name,
  category,
  keywords,
});

describe("emoji store", () => {
  describe("addEmojiToRecentsArray", () => {
    it("should add emoji to front of empty recents", () => {
      const emoji = createEmoji("1", "smile");
      const result = addEmojiToRecentsArray([], emoji);

      expect(result).toHaveLength(1);
      expect(result[0]).toEqual(emoji);
    });

    it("should add emoji to front of recents", () => {
      const existingEmoji = createEmoji("1", "smile");
      const newEmoji = createEmoji("2", "heart");

      const result = addEmojiToRecentsArray([existingEmoji], newEmoji);

      expect(result).toHaveLength(2);
      expect(result[0]).toEqual(newEmoji);
      expect(result[1]).toEqual(existingEmoji);
    });

    it("should limit recents to maxRecents (default MAX_RECENTS = 20)", () => {
      // Create 25 emojis
      const recents: Emoji[] = [];
      for (let i = 0; i < 25; i++) {
        recents.push(createEmoji(`existing-${i}`, `emoji-${i}`));
      }

      const newEmoji = createEmoji("new", "new-emoji");
      const result = addEmojiToRecentsArray(recents, newEmoji);

      expect(result).toHaveLength(MAX_RECENTS);
      expect(result[0]).toEqual(newEmoji);
    });

    it("should respect custom maxRecents limit", () => {
      const recents = [
        createEmoji("1", "one"),
        createEmoji("2", "two"),
        createEmoji("3", "three"),
      ];
      const newEmoji = createEmoji("new", "new-emoji");

      const result = addEmojiToRecentsArray(recents, newEmoji, 2);

      expect(result).toHaveLength(2);
      expect(result[0]).toEqual(newEmoji);
      expect(result[1].id).toBe("1");
    });

    it("should move existing emoji to front", () => {
      const emoji1 = createEmoji("1", "first");
      const emoji2 = createEmoji("2", "second");
      const emoji3 = createEmoji("3", "third");

      const recents = [emoji1, emoji2, emoji3];

      // Add emoji2 again - it should move to front
      const result = addEmojiToRecentsArray(recents, emoji2);

      expect(result).toHaveLength(3);
      expect(result[0]).toEqual(emoji2);
      expect(result[1]).toEqual(emoji1);
      expect(result[2]).toEqual(emoji3);
    });

    it("should not duplicate emoji when adding existing one", () => {
      const emoji1 = createEmoji("1", "first");
      const emoji2 = createEmoji("2", "second");

      const recents = [emoji1, emoji2];

      // Add emoji1 again
      const result = addEmojiToRecentsArray(recents, emoji1);

      expect(result).toHaveLength(2);
      // emoji1 should appear only once, at the front
      expect(result.filter((e) => e.id === "1")).toHaveLength(1);
      expect(result[0]).toEqual(emoji1);
    });
  });

  describe("searchEmojisInArray", () => {
    const emojis: Emoji[] = [
      createEmoji("smile", "smile", "smileys", ["happy", "face"]),
      createEmoji("heart", "red heart", "symbols", ["love", "romance"]),
      createEmoji("thumbsup", "thumbs up", "gestures", [
        "like",
        "approve",
        "ok",
      ]),
      createEmoji("fire", "fire", "nature", ["hot", "lit", "flame"]),
      createEmoji("cry", "crying face", "smileys", ["sad", "tears"]),
    ];

    it("should return all emojis when query is empty", () => {
      const result = searchEmojisInArray(emojis, "");
      expect(result).toHaveLength(emojis.length);
    });

    it("should return all emojis when query is whitespace", () => {
      const result = searchEmojisInArray(emojis, "   ");
      expect(result).toHaveLength(emojis.length);
    });

    it("should find emoji by exact name", () => {
      const result = searchEmojisInArray(emojis, "fire");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("fire");
    });

    it("should find emoji by partial name match", () => {
      const result = searchEmojisInArray(emojis, "heart");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("heart");
    });

    it("should find emoji by keyword", () => {
      const result = searchEmojisInArray(emojis, "happy");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("smile");
    });

    it("should find multiple emojis matching query", () => {
      // "face" should match "smile" (name: smile, keyword: face) and "cry" (name: crying face)
      const result = searchEmojisInArray(emojis, "face");

      expect(result).toHaveLength(2);
      const ids = result.map((e) => e.id);
      expect(ids).toContain("smile");
      expect(ids).toContain("cry");
    });

    it("should be case insensitive", () => {
      const result = searchEmojisInArray(emojis, "FIRE");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("fire");
    });

    it("should handle mixed case in keywords", () => {
      const emojisWithMixedCase: Emoji[] = [
        createEmoji("test", "Test Emoji", "test", ["MixedCase", "UPPERCASE"]),
      ];

      const result1 = searchEmojisInArray(emojisWithMixedCase, "mixedcase");
      expect(result1).toHaveLength(1);

      const result2 = searchEmojisInArray(emojisWithMixedCase, "uppercase");
      expect(result2).toHaveLength(1);
    });

    it("should return empty array when no matches found", () => {
      const result = searchEmojisInArray(emojis, "nonexistent");

      expect(result).toHaveLength(0);
    });

    it("should handle emojis without keywords", () => {
      const emojisNoKeywords: Emoji[] = [
        { id: "1", name: "simple", category: "test" },
        { id: "2", name: "another", category: "test" },
      ];

      const result = searchEmojisInArray(emojisNoKeywords, "simple");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("1");
    });

    it("should trim whitespace from query", () => {
      const result = searchEmojisInArray(emojis, "  fire  ");

      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("fire");
    });
  });
});
