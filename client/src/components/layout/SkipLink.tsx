/**
 * SkipLink — "Skip to content" (NAV-21, A11Y-19). The first stop for
 * keyboard users: jumps past the server and channel lists to the page's
 * <main> and moves keyboard focus there.
 */

import { Component } from "solid-js";
import { t } from "@/i18n";

const SkipLink: Component = () => (
  <a
    href="#main-content"
    onClick={(e) => {
      const target =
        document.getElementById("main-content") ?? document.querySelector<HTMLElement>("main");
      if (!target) return;
      e.preventDefault();
      if (!target.hasAttribute("tabindex")) target.setAttribute("tabindex", "-1");
      target.focus();
    }}
    class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[200] focus:px-4 focus:py-2 focus:bg-accent-primary focus:text-on-accent focus:rounded-lg focus:text-sm focus:font-medium"
  >
    {t("common.skipToContent")}
  </a>
);

export default SkipLink;
