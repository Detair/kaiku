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

import { createEffect, createUniqueId, on } from "solid-js";

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

  const focusTab = (id: T) => document.getElementById(tabId(id))?.focus();

  const onKeyDown = (e: KeyboardEvent) => {
    // Alt+Left (browser back), Ctrl+Home and the like are not tab keys (K4).
    if (e.altKey || e.ctrlKey || e.metaKey) return;
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
    // Select first, move focus after: if the owner refuses or asks first
    // (unsaved changes), focus stays on the open tab and returns there when
    // the question is cancelled (K3).
    opts.onSelect(id);
    if (opts.active() === id) focusTab(id);
  };

  // A tab that opens later (after "Discard changes") takes focus from the
  // tab that was focused, so focus is never on an unselected tab.
  createEffect(
    on(
      () => opts.active(),
      (id) => {
        // After the current update: a closing question gives focus back to
        // its opener first, then the new tab takes it over.
        queueMicrotask(() => {
          const focused = document.activeElement;
          if (focused?.getAttribute("role") === "tab" && focused.id.startsWith(`${prefix}-tab-`)) {
            if (focused.id !== tabId(id)) focusTab(id);
          }
        });
      },
      { defer: true },
    ),
  );

  return {
    /** Put focus on the open tab (after a refused or cancelled switch). */
    focusActive: () => focusTab(opts.active()),
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
