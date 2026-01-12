/**
 * CodeBlock - Syntax-highlighted code block component
 *
 * Uses highlight.js for syntax highlighting with automatic language detection.
 * Integrates with SolidMarkdown for rendering markdown code blocks.
 *
 * Supported languages: JavaScript, TypeScript, Python, Rust, JSON, Bash
 */

import { Component, onMount } from "solid-js";
import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import json from "highlight.js/lib/languages/json";
import bash from "highlight.js/lib/languages/bash";

// Register languages
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("json", json);
hljs.registerLanguage("bash", bash);

interface CodeBlockProps {
  /** Code content to highlight */
  children: string;
  /** Optional language hint (e.g., "rust", "typescript") */
  language?: string;
}

const CodeBlock: Component<CodeBlockProps> = (props) => {
  let codeRef: HTMLElement | undefined;

  onMount(() => {
    if (codeRef) {
      hljs.highlightElement(codeRef);
    }
  });

  return (
    <pre class="bg-surface-layer2 rounded-xl p-4 overflow-x-auto border border-white/5 my-3">
      <code
        ref={codeRef}
        class={props.language ? `language-${props.language}` : ""}
      >
        {props.children}
      </code>
    </pre>
  );
};

export default CodeBlock;
