// NAV-12: the phone drawer closes with Escape and has a close button.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import MobileDrawer from "../MobileDrawer";

afterEach(cleanup);

describe("MobileDrawer", () => {
  it("keeps the close button inside the drawer, so it is on screen at 320 px (N5)", () => {
    render(() => (
      <MobileDrawer open onClose={() => {}}>
        <div data-testid="content" />
      </MobileDrawer>
    ));
    const panel = screen.getByTestId("content").parentElement!.parentElement!;
    expect(panel.contains(screen.getByTestId("mobile-drawer-close"))).toBe(true);
    expect(panel.className).toContain("max-w-[100vw]");
  });

  it("closes with Escape and with the close button", () => {
    const onClose = vi.fn();
    render(() => (
      <MobileDrawer open onClose={onClose}>
        <div />
      </MobileDrawer>
    ));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("mobile-drawer-close"));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("ignores Escape while closed", () => {
    const onClose = vi.fn();
    render(() => (
      <MobileDrawer open={false} onClose={onClose}>
        <div />
      </MobileDrawer>
    ));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    // The close button sits in the drawer, which is closed (and inert).
    expect(screen.getByTestId("mobile-drawer-close").closest("[data-drawer-open]")?.getAttribute("data-drawer-open")).toBe("false");
  });
});
