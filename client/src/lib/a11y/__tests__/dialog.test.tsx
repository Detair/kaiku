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

describe("createDialog focus loss and stacking", () => {
  it("click backdrop (focus on body), then Escape closes the dialog and does not reach document listeners", () => {
    const onClose = vi.fn();
    const docListener = vi.fn();
    document.addEventListener("keydown", docListener);
    try {
      render(() => <TestDialog onClose={onClose} />);
      (document.activeElement as HTMLElement | null)?.blur();
      expect(document.activeElement).toBe(document.body);
      fireEvent.keyDown(document.body, { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(docListener).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", docListener);
    }
  });

  it("nested dialogs: Escape closes only the top one", () => {
    const outer = vi.fn();
    const inner = vi.fn();
    render(() => (
      <>
        <TestDialog onClose={outer} />
        <TestDialog onClose={inner} />
      </>
    ));
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });

  it("Tab with focus on body moves into the dialog", () => {
    render(() => <TestDialog onClose={() => {}} />);
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByText("first"));
  });

  it("non-Escape keys still reach descendant handlers", () => {
    const onKey = vi.fn();
    render(() => (
      <div role="dialog" tabIndex={-1} ref={createDialog({ onClose: () => {} }).ref}>
        <input aria-label="field" onKeyDown={onKey} />
      </div>
    ));
    fireEvent.keyDown(screen.getByLabelText("field"), { key: "a" });
    expect(onKey).toHaveBeenCalledTimes(1);
  });

  it("honours a custom initialFocus and skips tabindex=-1 elements", async () => {
    function Custom() {
      let target: HTMLButtonElement | undefined;
      const d = createDialog({ onClose: () => {}, initialFocus: () => target });
      return (
        <div {...d.dialogProps} ref={d.ref}>
          <button type="button">a</button>
          <button type="button" tabIndex={-1}>skipme</button>
          <button type="button" ref={target}>custom</button>
        </div>
      );
    }
    render(() => <Custom />);
    await Promise.resolve();
    expect(document.activeElement).toBe(screen.getByText("custom"));
    fireEvent.keyDown(screen.getByText("custom"), { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByText("a"));
  });
});

describe("createDialog and other overlays", () => {
  it("Tab with focus inside a portaled listbox is not pulled into the dialog", () => {
    render(() => <TestDialog onClose={() => {}} />);
    const portal = document.createElement("div");
    portal.setAttribute("role", "listbox");
    portal.innerHTML = '<button type="button" id="opt">opt</button>';
    document.body.appendChild(portal);
    try {
      portal.querySelector<HTMLElement>("#opt")!.focus();
      const notPrevented = fireEvent.keyDown(portal.querySelector("#opt")!, { key: "Tab" });
      expect(notPrevented).toBe(true);
      expect(document.activeElement?.id).toBe("opt");
    } finally {
      portal.remove();
    }
  });
});
