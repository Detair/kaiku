import { describe, it, expect } from "vitest";
import { renderEmbedRich, embedColorHex } from "@/lib/embedSanitize";

describe("renderEmbedRich", () => {
  it("renders basic markdown", () => {
    const html = renderEmbedRich("**bold** text");
    expect(html).toContain("<strong>bold</strong>");
  });

  it("strips <script> tags", () => {
    const html = renderEmbedRich("hi <script>alert(1)</script>");
    expect(html).not.toContain("<script");
    expect(html).not.toContain("alert(1)");
  });

  it("strips event-handler attributes", () => {
    const html = renderEmbedRich('<img src=x onerror="alert(2)">');
    expect(html.toLowerCase()).not.toContain("onerror");
  });

  it("drops disallowed tags like <iframe>", () => {
    const html = renderEmbedRich('<iframe src="https://evil"></iframe>');
    expect(html).not.toContain("<iframe");
  });
});

describe("embedColorHex", () => {
  it("formats a 24-bit color", () => {
    expect(embedColorHex(0xff0000)).toBe("#ff0000");
    expect(embedColorHex(0x0000ff)).toBe("#0000ff");
  });

  it("masks bits above 24", () => {
    expect(embedColorHex(0xffabcdef)).toBe("#abcdef");
  });

  it("falls back to accent for undefined", () => {
    expect(embedColorHex(undefined)).toContain("var(");
  });
});
