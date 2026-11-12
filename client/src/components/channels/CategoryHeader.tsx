/**
 * CategoryHeader - Collapsible category header in the channel sidebar
 *
 * Displays a category or subcategory with:
 * - Expand/collapse toggle (chevron icon)
 * - Category name (ALL CAPS for top-level, Title Case for subcategories)
 * - Unread indicator when collapsed and has unread channels
 * - Hover actions (create channel, settings)
 */

import { Component, Show } from "solid-js";
import { ChevronDown, ChevronRight, Plus, Settings, Hash, Volume2 } from "lucide-solid";
import type { CategoryType } from "@/lib/types";
import { t } from "@/i18n";

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
  /** Category type restriction */
  categoryType?: CategoryType;
  /** Callback when expand/collapse is toggled */
  onToggle: () => void;
  /** Callback when "create channel" button is clicked (only shown if provided) */
  onCreateChannel?: () => void;
  /** Callback when "settings" button is clicked (only shown if provided) */
  onSettings?: () => void;
}

const CategoryHeader: Component<CategoryHeaderProps> = (props) => {
  return (
    <div
      class={`flex items-center gap-1 select-none group rounded-lg hover:bg-white/5 transition-colors ${
        props.isSubcategory ? "ml-3 border-l-2 border-white/10" : ""
      }`}
    >
      {/* A real button, so a collapsed category opens by keyboard and its
          state is announced (NAV-20) */}
      <button
        type="button"
        data-testid="category-toggle"
        class={`flex items-center gap-1 flex-1 min-w-0 px-2 py-1 rounded-lg text-left cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/50 ${
          props.isSubcategory ? "pl-2" : ""
        }`}
        aria-expanded={!props.collapsed}
        onClick={() => props.onToggle()}
      >
        {/* Expand/Collapse chevron */}
        <span class="text-text-secondary w-3 transition-transform duration-200" aria-hidden="true">
          {props.collapsed ? (
            <ChevronRight class="w-3 h-3" />
          ) : (
            <ChevronDown class="w-3 h-3" />
          )}
        </span>

        {/* Category name */}
        <span
          class={`text-xs font-semibold tracking-wide flex-1 truncate transition-colors group-hover:text-text-primary ${
            props.isSubcategory
              ? "text-text-secondary"
              : "text-text-secondary uppercase"
          }`}
        >
          {props.name}
        </span>

        {/* Category type icon */}
        <Show when={props.categoryType === "text"}>
          <span title={t("channels.category.textOnly")}><Hash class="w-3 h-3 text-text-muted shrink-0" aria-hidden="true" /></span>
        </Show>
        <Show when={props.categoryType === "voice"}>
          <span title={t("channels.category.voiceOnly")}><Volume2 class="w-3 h-3 text-text-muted shrink-0" aria-hidden="true" /></span>
        </Show>

        {/* Unread indicator - shown when collapsed and has unread */}
        <Show when={props.hasUnread && props.collapsed}>
          <span class="w-2 h-2 rounded-full bg-white" aria-hidden="true" />
          <span class="sr-only">{t("channels.category.hasUnread")}</span>
        </Show>
      </button>

      {/* Actions: on hover, keyboard focus and touch screens (NAV-19) */}
      <Show when={props.onCreateChannel || props.onSettings}>
        <div class="flex items-center gap-1 pr-1 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 touch:opacity-100 transition-opacity">
          <Show when={props.onCreateChannel}>
            <button
              type="button"
              class="tap-target p-0.5 text-text-secondary hover:text-text-primary rounded hover:bg-white/10 transition-all duration-200"
              onClick={() => props.onCreateChannel?.()}
              title={t("channels.category.createChannel")} aria-label={t("channels.category.createChannel")}
            >
              <Plus class="w-3.5 h-3.5" aria-hidden="true" />
            </button>
          </Show>
          <Show when={props.onSettings}>
            <button
              type="button"
              class="tap-target p-0.5 text-text-secondary hover:text-text-primary rounded hover:bg-white/10 transition-all duration-200"
              onClick={() => props.onSettings?.()}
              title={t("channels.category.settings")} aria-label={t("channels.category.settings")}
            >
              <Settings class="w-3.5 h-3.5" aria-hidden="true" />
            </button>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default CategoryHeader;
