/**
 * CollapsibleModule Component
 *
 * Generic wrapper for collapsible sidebar modules.
 */

import { Component, JSX, Show } from "solid-js";
import { ChevronDown, ChevronRight } from "lucide-solid";
import { preferences, updateNestedPreference } from "@/stores/preferences";

interface CollapsibleModuleProps {
  id: "activeNow" | "pending" | "pins";
  title: string;
  badge?: number;
  children: JSX.Element;
}

const CollapsibleModule: Component<CollapsibleModuleProps> = (props) => {
  // Get collapsed state from preferences
  const isCollapsed = () => preferences().homeSidebar?.collapsed?.[props.id] ?? false;

  const toggleCollapse = () => {
    const currentCollapsed = preferences().homeSidebar?.collapsed ?? {};
    updateNestedPreference("homeSidebar", "collapsed", {
      ...currentCollapsed,
      [props.id]: !isCollapsed(),
    });
  };

  return (
    <div class="border-b border-white/10 last:border-b-0">
      {/* Header */}
      <button
        onClick={toggleCollapse}
        class="w-full flex items-center justify-between px-4 py-3 hover:bg-white/5 transition-colors"
      >
        <div class="flex items-center gap-2">
          <Show when={isCollapsed()} fallback={<ChevronDown class="w-4 h-4 text-text-secondary" />}>
            <ChevronRight class="w-4 h-4 text-text-secondary" />
          </Show>
          <span class="font-semibold text-text-primary">{props.title}</span>
          <Show when={props.badge && props.badge > 0}>
            <span class="px-1.5 py-0.5 text-xs font-medium bg-accent-primary text-white rounded-full">
              {props.badge}
            </span>
          </Show>
        </div>
      </button>

      {/* Content */}
      <Show when={!isCollapsed()}>
        <div class="px-4 pb-4 animate-in slide-in-from-top-2 duration-150">
          {props.children}
        </div>
      </Show>
    </div>
  );
};

export default CollapsibleModule;
