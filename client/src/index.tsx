/* @refresh reload */
import { render } from "solid-js/web";
import { Router } from "@solidjs/router";
import "virtual:uno.css";
import "@unocss/reset/tailwind.css";
import "./styles/global.css";
import AppRoutes from "./App";

const root = document.getElementById("root");

if (root) {
  render(
    () => (
      <Router>
        <AppRoutes />
      </Router>
    ),
    root
  );
}
