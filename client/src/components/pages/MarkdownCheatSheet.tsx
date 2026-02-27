/**
 * Markdown Cheat Sheet Component
 *
 * Quick reference panel for markdown syntax.
 */

import { Show, createSignal } from "solid-js";
import { ChevronDown, ChevronUp } from "lucide-solid";

interface MarkdownCheatSheetProps {
  class?: string;
}

export default function MarkdownCheatSheet(props: MarkdownCheatSheetProps) {
  const [isExpanded, setIsExpanded] = createSignal(false);

  const sections = [
    {
      title: "Text Formatting",
      items: [
        { syntax: "**bold**", description: "Bold text" },
        { syntax: "*italic*", description: "Italic text" },
        { syntax: "~~strikethrough~~", description: "Strikethrough" },
        { syntax: "`code`", description: "Inline code" },
      ],
    },
    {
      title: "Headers",
      items: [
        { syntax: "# H1", description: "Heading 1" },
        { syntax: "## H2", description: "Heading 2" },
        { syntax: "### H3", description: "Heading 3" },
      ],
    },
    {
      title: "Lists",
      items: [
        { syntax: "- item", description: "Bullet list" },
        { syntax: "1. item", description: "Numbered list" },
        { syntax: "- [ ] task", description: "Task list" },
      ],
    },
    {
      title: "Links & Images",
      items: [
        { syntax: "[text](url)", description: "Link" },
        { syntax: "![alt](url)", description: "Image" },
      ],
    },
    {
      title: "Blocks",
      items: [
        { syntax: "> quote", description: "Blockquote" },
        { syntax: "```lang\\ncode\\n```", description: "Code block" },
        { syntax: "---", description: "Horizontal rule" },
      ],
    },
    {
      title: "Tables",
      items: [
        { syntax: "| A | B |\\n|---|---|\\n| 1 | 2 |", description: "Table" },
      ],
    },
    {
      title: "Diagrams",
      items: [
        {
          syntax: "```mermaid\\ngraph LR\\n  A --> B\\n```",
          description: "Mermaid diagram",
        },
      ],
    },
  ];

  return (
    <div
      class={`bg-zinc-800 rounded-lg border border-zinc-700 ${props.class || ""}`}
    >
      <button
        type="button"
        class="w-full px-4 py-2 flex items-center justify-between text-sm text-zinc-300 hover:bg-zinc-700/50 rounded-lg transition-colors"
        onClick={() => setIsExpanded(!isExpanded())}
      >
        <span class="font-medium">Markdown Reference</span>
        <Show when={isExpanded()} fallback={<ChevronDown class="w-4 h-4" />}>
          <ChevronUp class="w-4 h-4" />
        </Show>
      </button>

      <Show when={isExpanded()}>
        <div class="px-4 pb-4 space-y-4">
          {sections.map((section) => (
            <div>
              <h4 class="text-xs font-semibold text-zinc-400 uppercase tracking-wide mb-2">
                {section.title}
              </h4>
              <div class="space-y-1">
                {section.items.map((item) => (
                  <div class="flex items-start gap-3 text-xs">
                    <code class="bg-zinc-900 px-1.5 py-0.5 rounded text-emerald-400 font-mono whitespace-pre">
                      {item.syntax}
                    </code>
                    <span class="text-zinc-400">{item.description}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </Show>
    </div>
  );
}
