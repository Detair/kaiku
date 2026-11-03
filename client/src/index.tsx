import { initSentry } from "@/lib/sentry";
initSentry();

/* @refresh reload */
import { render } from "solid-js/web";
import { Router } from "@solidjs/router";
import "@unocss/reset/tailwind.css";
import "virtual:uno.css";
import "./styles/global.css";
import "./styles/themes-structure.css";
import "./styles/themes.css";
import "./styles/themes-pixel.css";
import "./styles/highlight-theme.css";
import "./styles/themes-pixel-highlight.css";
import AppRoutes from "./App";
import { applyInitialTheme } from "./stores/theme";
import { initLocale, lazyAreasLoaded } from "./i18n";
import { StartupGate } from "./components/ui/ChunkLoadNotice";

applyInitialTheme();
// Load the stored UI language before the first render (never rejects).
await initLocale();

const root = document.getElementById("root");

if (root) {
  render(
    () => (
      // Never an app with blank texts: a missing text area gets a reload.
      <StartupGate ready={lazyAreasLoaded()}>
        <Router>
          <AppRoutes />
        </Router>
      </StartupGate>
    ),
    root,
  );
}
