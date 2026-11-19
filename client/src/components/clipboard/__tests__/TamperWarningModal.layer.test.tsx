import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import TamperWarningModal from "../TamperWarningModal";
import Modal from "@/components/ui/Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("TamperWarningModal layer", () => {
  it("opens in front of a window that is already open", () => {
    render(() => (
      <Modal onClose={() => {}} label="Earlier window" layer={MODAL_LAYER.prompt}>
        <button type="button">inside</button>
      </Modal>
    ));
    const earlier = screen.getByRole("dialog", { name: "Earlier window" });
    render(() => (
      <TamperWarningModal context="invite_link" onCancel={() => {}} onPasteAnyway={() => {}} />
    ));
    const buttons = screen.getAllByRole("button", { hidden: true });
    const cancel = buttons[buttons.length - 1];
    expect(zOf(cancel)).toBeGreaterThan(zOf(earlier));
  });
});
