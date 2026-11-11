// NAV-12: the phone drawer closes with Escape and has a close button.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import MobileDrawer from "../MobileDrawer";

afterEach(cleanup);

describe("MobileDrawer", () => {
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
    expect(screen.queryByTestId("mobile-drawer-close")).toBeNull();
  });
});
