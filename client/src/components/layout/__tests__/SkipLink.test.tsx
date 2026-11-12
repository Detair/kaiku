// NAV-21, A11Y-19: the skip link moves keyboard focus to the page content.
import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import SkipLink from "../SkipLink";

describe("SkipLink", () => {
  it("is translated and puts focus on <main>, past the lists", () => {
    document.body.innerHTML = "";
    const main = document.createElement("main");
    main.id = "main-content";
    main.tabIndex = -1;
    document.body.appendChild(main);
    render(() => <SkipLink />);
    const link = screen.getByRole("link", { name: "Skip to content" });
    link.focus();
    fireEvent.click(link);
    expect(document.activeElement).toBe(main);
  });

  it("falls back to any <main> (sign-in pages) and makes it focusable", () => {
    document.body.innerHTML = "";
    const main = document.createElement("main");
    document.body.appendChild(main);
    render(() => <SkipLink />);
    fireEvent.click(screen.getByRole("link", { name: "Skip to content" }));
    expect(document.activeElement).toBe(main);
  });
});
