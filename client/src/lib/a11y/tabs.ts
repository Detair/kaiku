/**
 * Tabs keyboard and semantics (KONTO-10).
 *
 * `createTabs` turns a row or column of buttons into the WAI-ARIA tabs
 * pattern: `tablist` / `tab` with `aria-selected` and `aria-controls`, one tab
 * stop (roving tabindex on the open tab), arrow keys move to the next or
 * previous tab and open it, Home and End jump to the first and last. The
 * content area is the matching `tabpanel`, named by its tab.
 *
 * Spread `listProps` on the list, `tabProps(id)` on each tab button and
 * `panelProps()` on the content area.
 */

import { createUniqueId } from "solid-js";

export interface TabsOptions<T extends string> {
  /** Visible tabs in order. */
  ids: () => T[];
  active: () => T;
  /** Open a tab. May refuse (unsaved changes): focus still moves. */
  onSelect: (id: T) => void;
  orientation?: "horizontal" | "vertical";
  /** Accessible name of the list. */
  label?: string;
}

export function createTabs<T extends string>(opts: TabsOptions<T>) {
  const prefix = `tabs-${createUniqueId()}`;
  const tabId = (id: T) => `${prefix}-tab-${id}`;
  const panelId = `${prefix}-panel`;
  const vertical = opts.orientation === "vertical";

  const onKeyDown = (e: KeyboardEvent) => {
    const ids = opts.ids();
    if (ids.length === 0) return;
    const focused = (e.target as HTMLElement | null)?.closest<HTMLElement>("[role=tab]");
    const current = ids.findIndex((id) => tabId(id) === focused?.id);
    const from = current >= 0 ? current : Math.max(0, ids.indexOf(opts.active()));
    const next = vertical ? "ArrowDown" : "ArrowRight";
    const prev = vertical ? "ArrowUp" : "ArrowLeft";
    let to: number;
    if (e.key === next) to = (from + 1) % ids.length;
    else if (e.key === prev) to = (from - 1 + ids.length) % ids.length;
    else if (e.key === "Home") to = 0;
    else if (e.key === "End") to = ids.length - 1;
    else return;
    e.preventDefault();
    const id = ids[to];
    document.getElementById(tabId(id))?.focus();
    opts.onSelect(id);
  };

  return {
    listProps: {
      role: "tablist" as const,
      "aria-orientation": (vertical ? "vertical" : "horizontal") as "vertical" | "horizontal",
      get "aria-label"() {
        return opts.label;
      },
      onKeyDown,
    },
    tabProps: (id: T) => ({
      role: "tab" as const,
      id: tabId(id),
      type: "button" as const,
      get "aria-selected"() {
        return opts.active() === id;
      },
      get "aria-controls"() {
        return opts.active() === id ? panelId : undefined;
      },
      get tabIndex() {
        return opts.active() === id ? 0 : -1;
      },
    }),
    panelProps: () => ({
      role: "tabpanel" as const,
      id: panelId,
      get "aria-labelledby"() {
        return tabId(opts.active());
      },
      tabIndex: 0,
    }),
  };
}
