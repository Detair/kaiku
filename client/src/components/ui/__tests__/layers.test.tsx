/**
 * Stacking order (UX4b fix round, Codex minor): toasts and notices sit above
 * every dialog, however many are stacked; context menus sit above toasts.
 */
import { describe, it, expect } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import Modal from "../Modal";
import { ToastContainer, showToast } from "../Toast";
import ContextMenuContainer, { showContextMenuAt } from "../ContextMenu";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

const z = (el: Element) => {
  const inline = (el as HTMLElement).style.zIndex;
  if (inline) return Number(inline);
  const m = /\bz-\[?(\d+)\]?/.exec(el.getAttribute("class") ?? "");
  return m ? Number(m[1]) : NaN;
};

describe("layers", () => {
  it("a toast and a context menu stay above three stacked dialogs and the picture viewer", async () => {
    render(() => <ToastContainer />);
    render(() => <ContextMenuContainer />);
    render(() => (
      <>
        <Modal onClose={() => {}} label="One"><button>a</button></Modal>
        <Modal onClose={() => {}} label="Two"><button>b</button></Modal>
        <Modal onClose={() => {}} label="Three" layer={MODAL_LAYER.dialog + 30}><button>c</button></Modal>
      </>
    ));
    showToast({ type: "error", title: "Could not save" });
    showContextMenuAt(10, 10, [{ label: "Copy", action: () => {} }]);
    await Promise.resolve();

    const dialogs = ["One", "Two", "Three"].map((n) => z(screen.getByRole("dialog", { name: n }).parentElement!));
    const toast = z(document.querySelector("[aria-live=polite]")!);
    const menu = z(screen.getByTestId("context-menu"));
    for (const d of dialogs) expect(toast).toBeGreaterThan(d);
    expect(menu).toBeGreaterThan(toast);
    expect(menu).toBe(MODAL_LAYER.popup);
  });
});
