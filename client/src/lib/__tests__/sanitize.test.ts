/**
 * XSS Prevention Tests
 *
 * Tests for HTML sanitization to prevent XSS attacks in message rendering.
 * These tests verify that DOMPurify correctly strips malicious content
 * while preserving safe markdown rendering.
 */

import { describe, it, expect } from "vitest";
import DOMPurify from "dompurify";
import { marked } from "marked";

// Configure marked for GitHub Flavored Markdown (matching MessageItem.tsx)
marked.setOptions({
  breaks: true,
  gfm: true,
});

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

describe("XSS Prevention", () => {
  describe("Script Injection", () => {
    it("should remove script tags", () => {
      const malicious = '<script>alert("XSS")</script>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<script");
      expect(result).not.toContain("alert");
    });

    it("should remove script tags from markdown", () => {
      const malicious = 'Hello <script>alert("XSS")</script> World';
      const result = parseAndSanitize(malicious);
      expect(result).not.toContain("<script");
      expect(result).not.toContain("alert");
      expect(result).toContain("Hello");
      expect(result).toContain("World");
    });

    it("should remove event handlers", () => {
      const malicious = '<img src="x" onerror="alert(1)">';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("onerror");
      expect(result).not.toContain("alert");
      // img tag itself should be removed (not in allowed list)
      expect(result).not.toContain("<img");
    });

    it("should remove onclick handlers", () => {
      const malicious = '<a href="#" onclick="alert(1)">Click me</a>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("onclick");
      expect(result).not.toContain("alert");
      // a tag should remain but without onclick
      expect(result).toContain("<a");
      expect(result).toContain("Click me");
    });

    it("should remove onmouseover handlers", () => {
      const malicious = '<div onmouseover="alert(1)">Hover me</div>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("onmouseover");
      // div tag should be removed (not in allowed list)
      expect(result).not.toContain("<div");
      expect(result).toContain("Hover me");
    });
  });

  describe("JavaScript URLs", () => {
    it("should remove javascript: protocol in href", () => {
      const malicious = '<a href="javascript:alert(1)">Click</a>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("javascript:");
    });

    it("should remove javascript: protocol from markdown links", () => {
      const malicious = "[Click](javascript:alert(1))";
      const result = parseAndSanitize(malicious);
      expect(result).not.toContain("javascript:");
    });

    it("should remove data: URLs", () => {
      const malicious =
        '<a href="data:text/html,<script>alert(1)</script>">Click</a>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("data:");
    });

    it("should remove vbscript: protocol", () => {
      const malicious = '<a href="vbscript:msgbox(1)">Click</a>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("vbscript:");
    });
  });

  describe("Forbidden Tags", () => {
    it("should remove style tags", () => {
      const malicious = '<style>body { background: red; }</style>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<style");
      expect(result).not.toContain("background");
    });

    it("should remove iframe tags", () => {
      const malicious = '<iframe src="https://evil.com"></iframe>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<iframe");
      expect(result).not.toContain("evil.com");
    });

    it("should remove object tags", () => {
      const malicious = '<object data="evil.swf"></object>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<object");
    });

    it("should remove embed tags", () => {
      const malicious = '<embed src="evil.swf">';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<embed");
    });

    it("should remove form tags", () => {
      const malicious = '<form action="https://evil.com"><input></form>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<form");
      expect(result).not.toContain("<input");
    });

    it("should remove svg tags", () => {
      const malicious = '<svg onload="alert(1)"><circle></circle></svg>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<svg");
      expect(result).not.toContain("onload");
    });

    it("should remove math tags", () => {
      const malicious = "<math><mrow></mrow></math>";
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<math");
    });

    it("should remove img tags", () => {
      // img not in allowed list for message content
      const malicious = '<img src="image.jpg">';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<img");
    });
  });

  describe("Forbidden Attributes", () => {
    it("should remove style attributes", () => {
      const malicious = '<p style="background: red;">Text</p>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("style=");
      expect(result).toContain("<p>");
      expect(result).toContain("Text");
    });

    it("should remove id attributes", () => {
      const malicious = '<p id="malicious">Text</p>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain('id="');
    });

    it("should strip arbitrary class values but keep allowed ones", () => {
      const malicious = '<span class="evil admin-panel">Text</span>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("evil");
      expect(result).not.toContain("admin-panel");
      // class attribute removed entirely since no allowed classes remain
      expect(result).not.toContain('class=');
    });

    it("should preserve allowed class values", () => {
      const safe = '<span class="spoiler">Hidden</span>';
      const result = sanitizeHtml(safe);
      expect(result).toContain('class="spoiler"');
    });

    it("should filter mixed allowed and disallowed class values", () => {
      const mixed = '<span class="spoiler evil">Text</span>';
      const result = sanitizeHtml(mixed);
      expect(result).toContain('class="spoiler"');
      expect(result).not.toContain("evil");
    });

    it("should remove data-* attributes", () => {
      const malicious = '<p data-evil="payload">Text</p>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("data-");
    });

    it("should remove src attributes from non-allowed tags", () => {
      const malicious = '<p src="evil.js">Text</p>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain('src="');
    });
  });

  describe("Allowed Tags Preserved", () => {
    it("should preserve paragraph tags", () => {
      const safe = "<p>Hello World</p>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<p>");
      expect(result).toContain("Hello World");
    });

    it("should preserve bold/strong tags", () => {
      const safe = "<strong>Bold text</strong>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<strong>");
    });

    it("should preserve italic/em tags", () => {
      const safe = "<em>Italic text</em>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<em>");
    });

    it("should preserve code tags", () => {
      const safe = "<code>const x = 1;</code>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<code>");
    });

    it("should preserve pre tags", () => {
      const safe = "<pre>Preformatted text</pre>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<pre>");
    });

    it("should preserve links with safe href", () => {
      const safe = '<a href="https://example.com">Link</a>';
      const result = sanitizeHtml(safe);
      expect(result).toContain('<a href="https://example.com">');
    });

    it("should preserve lists", () => {
      const safe = "<ul><li>Item 1</li><li>Item 2</li></ul>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<ul>");
      expect(result).toContain("<li>");
    });

    it("should preserve blockquotes", () => {
      const safe = "<blockquote>Quote</blockquote>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<blockquote>");
    });

    it("should preserve headings", () => {
      const safe = "<h1>Heading</h1>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<h1>");
    });

    it("should preserve tables", () => {
      const safe = "<table><tr><td>Cell</td></tr></table>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<table>");
      expect(result).toContain("<tr>");
      expect(result).toContain("<td>");
    });

    it("should preserve strikethrough", () => {
      const safe = "<del>Deleted</del><s>Strike</s>";
      const result = sanitizeHtml(safe);
      expect(result).toContain("<del>");
      expect(result).toContain("<s>");
    });
  });

  describe("Markdown Rendering", () => {
    it("should render bold markdown", () => {
      const markdown = "**bold text**";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("<strong>");
      expect(result).toContain("bold text");
    });

    it("should render italic markdown", () => {
      const markdown = "*italic text*";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("<em>");
      expect(result).toContain("italic text");
    });

    it("should render safe links from markdown", () => {
      const markdown = "[Example](https://example.com)";
      const result = parseAndSanitize(markdown);
      expect(result).toContain('<a href="https://example.com"');
      expect(result).toContain("Example");
    });

    it("should render code blocks", () => {
      const markdown = "`inline code`";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("<code>");
      expect(result).toContain("inline code");
    });

    it("should render strikethrough markdown", () => {
      const markdown = "~~deleted~~";
      const result = parseAndSanitize(markdown);
      expect(result).toContain("<del>");
    });
  });

  describe("Complex XSS Payloads", () => {
    it("should handle SVG XSS", () => {
      const malicious =
        '<svg/onload=alert("XSS")>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<svg");
      expect(result).not.toContain("alert");
    });

    it("should handle img XSS with malformed attributes", () => {
      const malicious = '<img """><script>alert(1)</script>">';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<script");
      expect(result).not.toContain("alert");
    });

    it("should handle nested tags XSS", () => {
      const malicious = '<p><script>alert(1)</script></p>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("<script");
      expect(result).toContain("<p>");
    });

    it("should handle URL encoding XSS", () => {
      const malicious = '<a href="&#106;&#97;&#118;&#97;&#115;&#99;&#114;&#105;&#112;&#116;&#58;alert(1)">Click</a>';
      const result = sanitizeHtml(malicious);
      // DOMPurify handles HTML entity decoding and blocks javascript:
      expect(result).not.toContain("alert(1)");
    });

    it("should handle base64 data URI XSS", () => {
      const malicious =
        '<a href="data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==">Click</a>';
      const result = sanitizeHtml(malicious);
      expect(result).not.toContain("data:");
    });

    it("should handle event handler in markdown image", () => {
      // Even if markdown produces an img tag, DOMPurify strips it
      // because img is not in the allowed tags list
      const malicious = "![image](http://example.com/image.jpg)";
      const result = parseAndSanitize(malicious);
      // img tag should be completely removed (not in allowed list)
      expect(result).not.toContain("<img");
    });

    it("should handle malicious markdown link", () => {
      const malicious = '[Click](javascript:alert(1) "title")';
      const result = parseAndSanitize(malicious);
      expect(result).not.toContain("javascript:");
    });
  });

  describe("Edge Cases", () => {
    it("should handle empty string", () => {
      const result = sanitizeHtml("");
      expect(result).toBe("");
    });

    it("should handle plain text", () => {
      const text = "Hello, World!";
      const result = sanitizeHtml(text);
      expect(result).toBe(text);
    });

    it("should handle very long content", () => {
      const longContent = "<p>" + "a".repeat(100000) + "</p>";
      const result = sanitizeHtml(longContent);
      expect(result).toContain("<p>");
      expect(result.length).toBeGreaterThan(100000);
    });

    it("should handle special characters", () => {
      const content = "<p>&amp; &lt; &gt; &quot;</p>";
      const result = sanitizeHtml(content);
      expect(result).toContain("&amp;");
      expect(result).toContain("&lt;");
      expect(result).toContain("&gt;");
    });

    it("should handle unicode content", () => {
      const content = "<p>你好世界 🎉</p>";
      const result = sanitizeHtml(content);
      expect(result).toContain("你好世界");
      expect(result).toContain("🎉");
    });
  });
});
