import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";

vi.mock("@/stores/friends", () => ({ sendFriendRequest: vi.fn() }));

import AddFriend from "../AddFriend";
import Modal from "@/components/ui/Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("AddFriend layer", () => {
  it("opens in front of a window that is already open (it takes the keyboard)", async () => {
    // For example the first-run tour, which sits on the dialog layer.
    render(() => (
      <Modal onClose={() => {}} label="Earlier window" layer={MODAL_LAYER.dialog}>
        <button type="button">inside</button>
      </Modal>
    ));
    const earlier = screen.getByRole("dialog", { name: "Earlier window" });
    render(() => <AddFriend onClose={() => {}} />);
    await Promise.resolve();
    const later = screen.getAllByRole("dialog").find((d) => d !== earlier)!;
    expect(zOf(earlier)).toBeGreaterThanOrEqual(MODAL_LAYER.dialog);
    expect(zOf(later)).toBeGreaterThan(zOf(earlier));
  });
});
