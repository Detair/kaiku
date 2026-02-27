/**
 * Secure Markdown Preview Component
 *
 * Renders markdown with DOMPurify sanitization and Mermaid diagram support.
 *
 * Security: All HTML is sanitized using DOMPurify with a strict allowlist
 * before rendering. This is the recommended approach for rendering user-
 * generated markdown content safely.
 */

import { createSignal, createEffect, onMount, Show } from "solid-js";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import mermaid from "mermaid";

// Initialize mermaid with strict security
let mermaidInitialized = false;

function initMermaid() {
  if (mermaidInitialized) return;

  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "dark",
    fontFamily: "inherit",
  });
  mermaidInitialized = true;
}

// DOMPurify configuration - allowlist approach (passed inline, not via setConfig)
const ALLOWED_TAGS = [
  "p",
  "br",
  "strong",
  "em",
  "b",
  "i",
  "u",
  "s",
  "del",
  "code",
  "pre",
  "kbd",
  "samp",
  "var",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "ul",
  "ol",
  "li",
  "dl",
  "dt",
  "dd",
  "blockquote",
  "hr",
  "a",
  "img",
  "table",
  "thead",
  "tbody",
  "tfoot",
  "tr",
  "th",
  "td",
  "div",
  "span",
  "sup",
  "sub",
  "details",
  "summary",
];

const ALLOWED_ATTR = [
  "href",
  "src",
  "alt",
  "title",
  "class",
  "id",
  "target",
  "rel",
  "width",
  "height",
  "colspan",
  "rowspan",
  "align",
  "open",
];

// URL protocol allowlist
const ALLOWED_URI_REGEXP =
  /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

/** Generate a GitHub-style slug from heading text, handling duplicates. */
function slugify(text: string, seen: Map<string, number>): string {
  const slug = text
    .toLowerCase()
    .trim()
    .replace(/[^\w\s-]/g, "")
    .replace(/\s+/g, "-");
  const count = seen.get(slug) ?? 0;
  seen.set(slug, count + 1);
  // Escape quotes for safe use in HTML attributes (defense-in-depth)
  const result = count === 0 ? slug : `${slug}-${count}`;
  return result.replace(/"/g, "");
}

// Note: Each MarkdownPreview instance creates its own Marked parser to avoid
// shared mutable state between simultaneous instances.

/** Add hover anchor links to headings in the rendered DOM. */
function addHeadingAnchors(container: HTMLElement): void {
  const headings = container.querySelectorAll(
    "h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]",
  );
  for (const heading of headings) {
    // Skip if anchor already added
    if (heading.querySelector(".heading-anchor")) continue;

    const id = heading.getAttribute("id");
    if (!id) continue;

    const anchor = document.createElement("a");
    anchor.className = "heading-anchor";
    anchor.href = `#${id}`;
    anchor.textContent = "#";
    anchor.setAttribute("aria-label", `Link to ${heading.textContent}`);
    anchor.style.cssText =
      "margin-left:0.5em;color:#71717a;text-decoration:none;opacity:0;transition:opacity 0.15s;font-weight:normal;";
    anchor.addEventListener("click", (e) => {
      e.preventDefault();
      const url = new URL(window.location.href);
      url.hash = id;
      navigator.clipboard.writeText(url.toString()).catch((err) => {
        console.warn("Failed to copy heading URL to clipboard:", err);
      });
      history.replaceState(null, "", url.toString());
      heading.scrollIntoView({ behavior: "smooth" });
    });
    (heading as HTMLElement).addEventListener("mouseenter", () => {
      anchor.style.opacity = "1";
    });
    (heading as HTMLElement).addEventListener("mouseleave", () => {
      anchor.style.opacity = "0";
    });
    heading.appendChild(anchor);
  }
}

// Inline config passed to each sanitize() call — avoids global setConfig pollution
const MARKDOWN_PURIFY_CONFIG = {
  ALLOWED_TAGS,
  ALLOWED_ATTR,
  ALLOWED_URI_REGEXP,
  ALLOW_DATA_ATTR: false,
  ADD_ATTR: ["target"],
};

// Global hook: opens external links in new tab with noopener.
// Intentionally global — this behavior is desirable for all sanitized contexts.
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    const href = node.getAttribute("href") || "";
    // External links open in new tab
    if (href.startsWith("http://") || href.startsWith("https://")) {
      node.setAttribute("target", "_blank");
      node.setAttribute("rel", "noopener noreferrer");
    }
  }
});

// SVG-specific sanitization for mermaid diagrams — explicit allowlist (not additive).
// foreignObject, script, and style are excluded as XSS/CSS-injection vectors.
// DOMPurify does NOT sanitize CSS content inside <style> elements, so allowing
// <style> would permit arbitrary CSS injection (data exfiltration via url(),
// UI redressing via position:fixed). Mermaid diagrams render via inline SVG
// presentation attributes instead.
const MERMAID_SVG_CONFIG = {
  ALLOWED_TAGS: [
    "svg",
    "g",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "defs",
    "marker",
    "clipPath",
    "title",
    "desc",
  ],
  ALLOWED_ATTR: [
    "viewBox",
    "d",
    "fill",
    "stroke",
    "stroke-width",
    "transform",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "points",
    "font-size",
    "font-family",
    "text-anchor",
    "dominant-baseline",
    "marker-end",
    "marker-start",
    "xmlns",
    "preserveAspectRatio",
    "class",
    "id",
    "width",
    "height",
    "clip-path",
    "opacity",
  ],
  FORBID_TAGS: ["foreignObject", "script", "style"],
  ALLOW_DATA_ATTR: false,
};

interface MarkdownPreviewProps {
  content: string;
  class?: string;
}

export default function MarkdownPreview(props: MarkdownPreviewProps) {
  const [html, setHtml] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [renderVersion, setRenderVersion] = createSignal(0);
  let containerRef: HTMLDivElement | undefined;

  // Per-component Marked instance avoids global state mutation between instances
  const parser = new Marked();

  onMount(() => {
    initMermaid();
  });

  // Parse markdown and sanitize
  // Uses version counter to prevent race conditions with rapid content changes
  createEffect(async () => {
    const content = props.content;
    const version = renderVersion() + 1;
    setRenderVersion(version);

    if (!content) {
      setHtml("");
      setError(null);
      return;
    }

    try {
      // Configure renderer with a fresh slug map per-parse
      const slugMap = new Map<string, number>();
      parser.use({
        renderer: {
          heading({ text, depth }: { text: string; depth: number }) {
            const id = slugify(text, slugMap);
            return `<h${depth} id="${id}">${text}</h${depth}>`;
          },
        },
      });
      const rawHtml = await parser.parse(content, {
        gfm: true,
        breaks: true,
      });

      // Check if content changed during async operation (race condition guard)
      if (renderVersion() !== version) {
        return; // Stale render, skip update
      }

      // Sanitize HTML using DOMPurify with inline config (not global setConfig)
      const sanitizedHtml = DOMPurify.sanitize(rawHtml, MARKDOWN_PURIFY_CONFIG);
      setHtml(sanitizedHtml);
      setError(null);
    } catch (err) {
      // Only update error if this is still the current render
      if (renderVersion() === version) {
        console.error("Markdown parsing error:", err);
        setError("Failed to render markdown");
      }
    }
  });

  // Render mermaid diagrams after HTML is set
  // Uses version tracking to prevent stale async renders from modifying DOM
  createEffect(() => {
    const currentHtml = html();
    const version = renderVersion();
    if (!currentHtml || !containerRef) return;

    // Find and render mermaid code blocks
    const codeBlocks = containerRef.querySelectorAll(
      "pre code.language-mermaid, pre code.mermaid",
    );

    codeBlocks.forEach(async (block, index) => {
      const code = block.textContent || "";
      const pre = block.parentElement;
      if (!pre || !code.trim()) return;

      try {
        // Use version + index for unique IDs (version is unique per render cycle)
        const id = `mermaid-v${version}-${index}`;
        const { svg } = await mermaid.render(id, code.trim());

        // Check if render is still current (race condition guard)
        if (renderVersion() !== version) {
          return; // DOM has been replaced, skip modification
        }

        // Replace the code block with sanitized SVG from mermaid
        const wrapper = document.createElement("div");
        wrapper.className = "mermaid-diagram";
        // Security: SVG is sanitized through DOMPurify with strict SVG-only allowlist
        wrapper.innerHTML = DOMPurify.sanitize(svg, MERMAID_SVG_CONFIG);
        pre.replaceWith(wrapper);
      } catch (err) {
        // Only show error if this is still the current render
        if (renderVersion() !== version) return;

        console.error("Mermaid rendering error:", err);
        // Leave the code block as-is if mermaid fails
        const errorDiv = document.createElement("div");
        errorDiv.className =
          "mermaid-error text-red-400 text-sm p-2 bg-red-900/20 rounded";
        errorDiv.textContent = "Failed to render diagram";
        pre.appendChild(errorDiv);
      }
    });
  });

  // Add anchor links to headings after HTML is rendered
  createEffect(() => {
    html(); // Track html changes
    if (!containerRef) return;
    // Small delay to ensure DOM is updated
    requestAnimationFrame(() => {
      if (containerRef) addHeadingAnchors(containerRef);
    });
  });

  return (
    <div class={`markdown-preview ${props.class || ""}`}>
      <Show when={error()}>
        <div class="text-red-400 text-sm p-2 bg-red-900/20 rounded mb-2">
          {error()}
        </div>
      </Show>
      {/*
        Security: innerHTML is safe here because all content is sanitized
        through DOMPurify with a strict allowlist before being set.
        This is the recommended pattern for rendering markdown.
      */}
      <div
        ref={containerRef}
        class="prose prose-invert prose-sm max-w-none"
        innerHTML={html()}
      />
    </div>
  );
}
