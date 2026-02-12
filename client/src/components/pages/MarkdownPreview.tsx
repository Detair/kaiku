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
import { marked } from "marked";
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
  "p", "br", "strong", "em", "b", "i", "u", "s", "del",
  "code", "pre", "kbd", "samp", "var",
  "h1", "h2", "h3", "h4", "h5", "h6",
  "ul", "ol", "li", "dl", "dt", "dd",
  "blockquote", "hr",
  "a", "img",
  "table", "thead", "tbody", "tfoot", "tr", "th", "td",
  "div", "span",
  "sup", "sub",
  "details", "summary",
];

const ALLOWED_ATTR = [
  "href", "src", "alt", "title", "class", "id",
  "target", "rel",
  "width", "height",
  "colspan", "rowspan", "align",
  "open",
];

// URL protocol allowlist
const ALLOWED_URI_REGEXP = /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

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

// SVG-specific sanitization for mermaid diagrams — explicit allowlist (not additive)
// foreignObject, style, and script are excluded as they are XSS vectors
const MERMAID_SVG_CONFIG = {
  ALLOWED_TAGS: ["svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "defs", "marker"],
  ALLOWED_ATTR: ["viewBox", "d", "fill", "stroke", "stroke-width", "transform", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "points", "font-size", "font-family", "text-anchor", "dominant-baseline", "marker-end", "marker-start", "xmlns", "preserveAspectRatio"],
  FORBID_TAGS: ["foreignObject", "style", "script"],
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
      // Parse markdown to HTML
      const rawHtml = await marked.parse(content, {
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
    const codeBlocks = containerRef.querySelectorAll("pre code.language-mermaid, pre code.mermaid");

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
        errorDiv.className = "mermaid-error text-red-400 text-sm p-2 bg-red-900/20 rounded";
        errorDiv.textContent = "Failed to render diagram";
        pre.appendChild(errorDiv);
      }
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
