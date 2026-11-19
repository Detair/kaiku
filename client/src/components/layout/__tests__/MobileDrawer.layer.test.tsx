import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import MobileDrawer from "../MobileDrawer";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("MobileDrawer layer", () => {
  it("opens in front of a window that is already open (it takes the keyboard)", async () => {
    // For example the first-run tour, which sits on the dialog layer.
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.dialog);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => (
      <MobileDrawer open onClose={() => {}}>
        <a href="#x">Channel</a>
      </MobileDrawer>
    ));
    await Promise.resolve();
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    expect(zOf(screen.getByRole("dialog"))).toBeGreaterThan(earlier);
  });
});
