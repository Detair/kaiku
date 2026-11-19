import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import LogoutConfirmDialog from "../LogoutConfirmDialog";
import Modal from "@/components/ui/Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

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
    render(() => (
      <Modal onClose={() => {}} label="Earlier window" layer={MODAL_LAYER.prompt}>
        <button type="button">inside</button>
      </Modal>
    ));
    const earlier = screen.getByRole("dialog", { name: "Earlier window" });
    render(() => <LogoutConfirmDialog onConfirm={() => {}} onCancel={() => {}} />);
    await Promise.resolve();
    const later = screen.getAllByRole("dialog").find((d) => d !== earlier)!;
    expect(zOf(earlier)).toBeGreaterThanOrEqual(MODAL_LAYER.prompt);
    expect(zOf(later)).toBeGreaterThan(zOf(earlier));
  });
});
