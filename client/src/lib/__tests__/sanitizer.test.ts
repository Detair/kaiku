/**
 * Tests for the isolated DOMPurify factory (createIsolatedPurifier).
 *
 * Covers the two properties the security-review fix relies on:
 *  1. The shared external-link hardening hook (target=_blank + rel=noopener)
 *     runs on every instance — message links must not lose this when the
 *     renderers stopped sharing one global DOMPurify.
 *  2. Instances are isolated — a hook added to one does not affect another.
 */

import { describe, it, expect } from "vitest";
import { createIsolatedPurifier } from "../sanitizer";

describe("createIsolatedPurifier", () => {
  it("hardens external links with target and rel=noopener", () => {
    const p = createIsolatedPurifier();
    const out = p.sanitize('<a href="https://evil.example/x">click</a>', {
      ALLOWED_TAGS: ["a"],
      ALLOWED_ATTR: ["href", "target", "rel"],
    });
    expect(out).toContain('target="_blank"');
    expect(out).toContain('rel="noopener noreferrer"');
  });

  it("does not add target/rel to non-http(s) (relative) links", () => {
    const p = createIsolatedPurifier();
    const out = p.sanitize('<a href="/channels/1">go</a>', {
      ALLOWED_TAGS: ["a"],
      ALLOWED_ATTR: ["href", "target", "rel"],
    });
    expect(out).not.toContain("target=");
    expect(out).not.toContain("rel=");
  });

  it("isolates hooks between instances", () => {
    const a = createIsolatedPurifier();
    const b = createIsolatedPurifier();

    // Register a class-stripping hook on instance A only.
    a.addHook("uponSanitizeAttribute", (_node, data) => {
      if (data.attrName === "class") {
        data.attrValue = "";
        data.keepAttr = false;
      }
    });

    const cfg = { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] };
    // A strips the class...
    expect(a.sanitize('<span class="keep">x</span>', cfg)).not.toContain(
      "class=",
    );
    // ...but B (isolated) is unaffected.
    expect(b.sanitize('<span class="keep">x</span>', cfg)).toContain(
      'class="keep"',
    );
  });

  it("still removes script/dangerous content (baseline XSS)", () => {
    const p = createIsolatedPurifier();
    const out = p.sanitize('<img src=x onerror=alert(1)><script>alert(2)</script>ok', {
      ALLOWED_TAGS: ["p"],
      ALLOWED_ATTR: [],
    });
    expect(out).not.toContain("onerror");
    expect(out).not.toContain("<script");
    expect(out).toContain("ok");
  });
});
