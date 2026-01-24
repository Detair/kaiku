/**
 * CategoryHeader - Collapsible category header in the channel sidebar
 *
 * Displays a category or subcategory with:
 * - Expand/collapse toggle (chevron icon)
 * - Category name (ALL CAPS for top-level, Title Case for subcategories)
 * - Unread indicator when collapsed and has unread channels
 * - Hover actions (create channel, settings)
 */

import { Component, createSignal, Show } from "solid-js";
import { ChevronDown, ChevronRight, Plus, Settings } from "lucide-solid";

interface CategoryHeaderProps {
  /** Category ID */
  id: string;
  /** Category name */
  name: string;
  /** Whether the category is currently collapsed */
  collapsed: boolean;
  /** Whether any channel in this category has unread messages */
  hasUnread: boolean;
  /** Whether this is a subcategory (nested under another category) */
  isSubcategory: boolean;
  /** Callback when expand/collapse is toggled */
  onToggle: () => void;
  /** Callback when "create channel" button is clicked (only shown if provided) */
  onCreateChannel?: () => void;
  /** Callback when "settings" button is clicked (only shown if provided) */
  onSettings?: () => void;
}

const CategoryHeader: Component<CategoryHeaderProps> = (props) => {
  const [hovering, setHovering] = createSignal(false);

  return (
    <div
      class={`flex items-center gap-1 px-2 py-1 cursor-pointer select-none group rounded-lg hover:bg-white/5 transition-colors ${
        props.isSubcategory ? "ml-3 border-l-2 border-white/10 pl-2" : ""
      }`}
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
      onClick={props.onToggle}
    >
      {/* Expand/Collapse chevron */}
      <span class="text-text-secondary w-3 transition-transform duration-200">
        {props.collapsed ? (
          <ChevronRight class="w-3 h-3" />
        ) : (
          <ChevronDown class="w-3 h-3" />
        )}
      </span>

      {/* Category name */}
      <span
        class={`text-xs font-semibold tracking-wide flex-1 transition-colors group-hover:text-text-primary ${
          props.isSubcategory
            ? "text-text-secondary"
            : "text-text-secondary uppercase"
        }`}
      >
        {props.name}
      </span>

      {/* Unread indicator - shown when collapsed and has unread */}
      <Show when={props.hasUnread && props.collapsed}>
        <span class="w-2 h-2 rounded-full bg-white" />
      </Show>

      {/* Hover actions */}
      <Show when={hovering()}>
        <div
          class="flex items-center gap-1"
          onClick={(e) => e.stopPropagation()}
        >
          <Show when={props.onCreateChannel}>
            <button
              class="p-0.5 text-text-secondary hover:text-text-primary rounded hover:bg-white/10 transition-all duration-200"
              onClick={props.onCreateChannel}
              title="Create Channel"
            >
              <Plus class="w-3.5 h-3.5" />
            </button>
          </Show>
          <Show when={props.onSettings}>
            <button
              class="p-0.5 text-text-secondary hover:text-text-primary rounded hover:bg-white/10 transition-all duration-200"
              onClick={props.onSettings}
              title="Category Settings"
            >
              <Settings class="w-3.5 h-3.5" />
            </button>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default CategoryHeader;
