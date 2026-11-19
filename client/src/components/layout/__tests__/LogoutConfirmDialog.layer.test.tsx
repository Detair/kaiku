import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import LogoutConfirmDialog from "../LogoutConfirmDialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("LogoutConfirmDialog layer", () => {
  it("opens in front of a prompt that is already open (a confirmation is a prompt)", async () => {
    // For example the full-screen open-request prompt, on the prompt layer.
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.prompt);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => <LogoutConfirmDialog onConfirm={() => {}} onCancel={() => {}} />);
    await Promise.resolve();
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    expect(zOf(screen.getByRole("dialog"))).toBeGreaterThan(earlier);
  });
});
