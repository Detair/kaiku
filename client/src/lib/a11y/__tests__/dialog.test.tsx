import { describe, it, expect, vi } from "vitest";
import { createSignal, Show } from "solid-js";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { createDialog } from "../dialog";

function TestDialog(props: { onClose: () => void }) {
  const d = createDialog({ onClose: props.onClose });
  return (
    <div {...d.dialogProps} ref={d.ref}>
      <h2 id={d.titleId}>Title</h2>
      <button type="button">first</button>
      <button type="button" disabled>
        skipped
      </button>
      <button type="button">last</button>
    </div>
  );
}

describe("createDialog", () => {
  it("Escape calls onClose once and does not reach document listeners", () => {
    const onClose = vi.fn();
    const docListener = vi.fn();
    document.addEventListener("keydown", docListener);
    try {
      render(() => <TestDialog onClose={onClose} />);
      fireEvent.keyDown(screen.getByText("first"), { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(docListener).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", docListener);
    }
  });

  it("exposes role dialog labelled by its title", () => {
    render(() => <TestDialog onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Title" })).toBeTruthy();
    expect(screen.getByRole("dialog").getAttribute("aria-modal")).toBe("true");
  });

  it("Tab from last focusable wraps to first", () => {
    render(() => <TestDialog onClose={() => {}} />);
    const first = screen.getByText("first");
    const last = screen.getByText("last");
    last.focus();
    const notPrevented = fireEvent.keyDown(last, { key: "Tab" });
    expect(notPrevented).toBe(false);
    expect(document.activeElement).toBe(first);
  });

  it("Shift+Tab from first focusable wraps to last", () => {
    render(() => <TestDialog onClose={() => {}} />);
    const first = screen.getByText("first");
    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getByText("last"));
  });

  it("moves focus into the dialog on open", async () => {
    render(() => <TestDialog onClose={() => {}} />);
    await Promise.resolve();
    expect(document.activeElement).toBe(screen.getByText("first"));
  });

  it("focus returns to the opener", async () => {
    function Host() {
      const [open, setOpen] = createSignal(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            open
          </button>
          <Show when={open()}>
            <TestDialog onClose={() => setOpen(false)} />
          </Show>
        </>
      );
    }
    render(() => <Host />);
    const opener = screen.getByText("open");
    opener.focus();
    opener.click();
    await Promise.resolve();
    expect(document.activeElement).toBe(screen.getByText("first"));
    fireEvent.keyDown(screen.getByText("first"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });
});

describe("isEscapeForOverlay", () => {
  it("is true for an Escape coming from inside a dialog", async () => {
    const { isEscapeForOverlay } = await import("../dialog");
    const host = document.createElement("div");
    host.innerHTML = '<div role="dialog"><button id="inner">x</button></div>';
    document.body.appendChild(host);
    let result: boolean | undefined;
    const spy = (e: KeyboardEvent) => {
      result = isEscapeForOverlay(e);
    };
    document.addEventListener("keydown", spy);
    try {
      host.querySelector("#inner")!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    } finally {
      document.removeEventListener("keydown", spy);
      host.remove();
    }
    expect(result).toBe(true);
  });

  it("is false with no overlay open", async () => {
    const { isEscapeForOverlay } = await import("../dialog");
    const e = new KeyboardEvent("keydown", { key: "Escape" });
    expect(isEscapeForOverlay(e)).toBe(false);
  });
});
