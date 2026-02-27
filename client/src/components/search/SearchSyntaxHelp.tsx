/**
 * SearchSyntaxHelp Component
 *
 * Small popover showing search query syntax examples.
 * Uses websearch_to_tsquery syntax (AND, OR, "exact phrase", -exclude).
 */

import {
  Component,
  Show,
  createSignal,
  onCleanup,
  createEffect,
} from "solid-js";
import { HelpCircle } from "lucide-solid";

const SearchSyntaxHelp: Component = () => {
  const [isOpen, setIsOpen] = createSignal(false);
  let popoverRef: HTMLDivElement | undefined;

  // Close on click outside
  const handleClickOutside = (e: MouseEvent) => {
    if (popoverRef && !popoverRef.contains(e.target as Node)) {
      setIsOpen(false);
    }
  };

  createEffect(() => {
    if (isOpen()) {
      document.addEventListener("mousedown", handleClickOutside);
      onCleanup(() =>
        document.removeEventListener("mousedown", handleClickOutside),
      );
    }
  });

  return (
    <div class="relative" ref={popoverRef}>
      <button
        onClick={() => setIsOpen(!isOpen())}
        class="ml-1 p-1.5 rounded transition-colors"
        classList={{
          "text-accent-primary bg-accent-primary/10": isOpen(),
          "text-text-secondary hover:text-text-primary": !isOpen(),
        }}
        title="Search syntax help"
      >
        <HelpCircle class="w-4 h-4" />
      </button>

      <Show when={isOpen()}>
        <div class="absolute right-0 top-full mt-1 w-64 rounded-lg border border-white/10 bg-surface-layer2 shadow-xl z-60 p-3">
          <h4 class="text-xs font-semibold text-text-primary mb-2">
            Search Syntax
          </h4>
          <table class="w-full text-xs">
            <tbody>
              <tr class="border-b border-white/5">
                <td class="py-1 pr-2 text-accent-primary font-mono">
                  hello world
                </td>
                <td class="py-1 text-text-secondary">Both words (AND)</td>
              </tr>
              <tr class="border-b border-white/5">
                <td class="py-1 pr-2 text-accent-primary font-mono">
                  hello OR world
                </td>
                <td class="py-1 text-text-secondary">Either word</td>
              </tr>
              <tr class="border-b border-white/5">
                <td class="py-1 pr-2 text-accent-primary font-mono">
                  "exact phrase"
                </td>
                <td class="py-1 text-text-secondary">Exact match</td>
              </tr>
              <tr>
                <td class="py-1 pr-2 text-accent-primary font-mono">
                  hello -world
                </td>
                <td class="py-1 text-text-secondary">Exclude word</td>
              </tr>
            </tbody>
          </table>
        </div>
      </Show>
    </div>
  );
};

export default SearchSyntaxHelp;
