import { initSentry } from "@/lib/sentry";
initSentry();

/* @refresh reload */
import { render } from "solid-js/web";
import { Router } from "@solidjs/router";
import "virtual:uno.css";
import "@unocss/reset/tailwind.css";
import "./styles/global.css";
import "./styles/themes-structure.css";
import "./styles/themes.css";
import "./styles/themes-pixel.css";
import "./styles/highlight-theme.css";
import "./styles/themes-pixel-highlight.css";
import AppRoutes from "./App";
import { applyInitialTheme } from "./stores/theme";

applyInitialTheme();

const root = document.getElementById("root");

if (root) {
  render(
    () => (
      <Router>
        <AppRoutes />
      </Router>
    ),
    root,
  );
}
