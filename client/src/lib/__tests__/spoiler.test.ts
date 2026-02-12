/**
 * Spoiler Syntax Tests
 *
 * Tests for spoiler rendering (||text||) to ensure proper parsing,
 * XSS prevention, and length limits.
 */

import { describe, it, expect } from "vitest";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { spoilerExtension } from "@/lib/markdown/spoilerExtension";

// Configure marked for GitHub Flavored Markdown
marked.setOptions({
  breaks: true,
  gfm: true,
});

marked.use({ extensions: [spoilerExtension] });

// DOMPurify config matching MessageItem.tsx
const PURIFY_CONFIG = {
  ALLOWED_TAGS: [
    "p", "br", "strong", "em", "code", "pre", "a", "ul", "ol", "li",
    "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "del", "s",
    "table", "thead", "tbody", "tr", "th", "td", "span", "mark",
  ],
  ALLOWED_ATTR: ["href", "target", "rel", "class", "data-spoiler"],
  ALLOW_DATA_ATTR: false,
  RETURN_TRUSTED_TYPE: false as const,
};

// Restrict class values to an allowlist (matching MessageItem.tsx hook)
const ALLOWED_CLASSES = new Set(["mention-everyone", "mention-user", "spoiler"]);
DOMPurify.addHook("uponSanitizeAttribute", (_node, data) => {
  if (data.attrName === "class") {
    const filtered = data.attrValue.split(/\s+/).filter(cls => ALLOWED_CLASSES.has(cls)).join(" ");
    data.attrValue = filtered;
    if (!filtered) data.keepAttr = false;
  }
});

const sanitizeHtml = (html: string): string => {
  return DOMPurify.sanitize(html, PURIFY_CONFIG) as string;
};

const parseAndSanitize = (markdown: string): string => {
  const html = marked.parse(markdown, { async: false }) as string;
  return sanitizeHtml(html);
};

describe("Spoiler Syntax", () => {
  describe("Basic Spoiler Rendering", () => {
    it("should render spoiler syntax as span with data attribute", () => {
      const markdown = "||hidden text||";
      const result = parseAndSanitize(markdown);
      expect(result).toContain('<span class="spoiler" data-spoiler="true">');
      expect(result).toContain("hidden text");
    });

    it("should render inline spoiler within text", () => {
      const markdown = "This is ||a secret|| message";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("This is");
      expect(result).toContain('<span class="spoiler"');
      expect(result).toContain("a secret");
      expect(result).toContain("message");
    });

    it("should render multiple spoilers in one message", () => {
      const markdown = "||spoiler 1|| and ||spoiler 2||";
      const result = parseAndSanitize(markdown);
      const spoilerCount = (result.match(/data-spoiler="true"/g) || []).length;
      expect(spoilerCount).toBe(2);
      expect(result).toContain("spoiler 1");
      expect(result).toContain("spoiler 2");
    });

    it("should handle spoiler with emoji", () => {
      const markdown = "||🎉 secret party||";
      const result = parseAndSanitize(markdown);
      expect(result).toContain('<span class="spoiler"');
      expect(result).toContain("🎉");
      expect(result).toContain("secret party");
    });
  });

  describe("Edge Cases", () => {
    it("should not render incomplete spoiler (missing closing)", () => {
      const markdown = "||incomplete spoiler";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("||incomplete spoiler");
      expect(result).not.toContain('data-spoiler="true"');
    });

    it("should handle empty spoiler", () => {
      const markdown = "||||";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("||||");
    });

    it("should handle nested pipes", () => {
      const markdown = "||text with | pipe||";
      const result = parseAndSanitize(markdown);
      expect(result).toContain('<span class="spoiler"');
      expect(result).toContain("text with | pipe");
    });
  });

  describe("Length Limit Protection (ReDoS Prevention)", () => {
    it("should handle spoilers up to 500 characters", () => {
      const longContent = "a".repeat(500);
      const markdown = `||${longContent}||`;
      const result = parseAndSanitize(markdown);
      expect(result).toContain('<span class="spoiler"');
      expect(result).toContain(longContent);
    });

    it("should NOT render spoilers over 500 characters", () => {
      const longContent = "a".repeat(501);
      const markdown = `||${longContent}||`;
      const result = parseAndSanitize(markdown);
      expect(result).not.toContain('data-spoiler="true"');
      expect(result).toContain("||");
    });
  });
});
