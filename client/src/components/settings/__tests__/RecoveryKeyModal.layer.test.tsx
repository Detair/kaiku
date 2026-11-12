import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/clipboard", () => ({ secureCopy: vi.fn() }));

import RecoveryKeyModal from "../RecoveryKeyModal";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("RecoveryKeyModal layer", () => {
  it("opens in front of a window that is already open (it takes the keyboard)", () => {
    // For example the first-run tour, which sits on the dialog layer.
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.dialog);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => (
      <RecoveryKeyModal
        keyChunks={["abcd", "efgh"]}
        fullKey="abcdefgh"
        onConfirm={() => {}}
        onClose={() => {}}
      />
    ));
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    expect(zOf(screen.getByRole("dialog"))).toBeGreaterThan(earlier);
  });
});
