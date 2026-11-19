import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { Show, Suspense, createSignal, lazy, type Component } from "solid-js";
import LazyDialog, { LOADING_DELAY_MS } from "../LazyDialog";
import Modal from "../Modal";
import { lazyWindow } from "@/lib/chunkLoadError";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

const quiet = () => vi.spyOn(console, "error").mockImplementation(() => {});

/** A window whose code cannot be loaded; WebKit's wording matches no pattern. */
const brokenWindow = () =>
  lazyWindow<Component>(() =>
    Promise.reject(new TypeError("'text/html' is not a valid JavaScript MIME type.")),
  );

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

/** A button that opens the window; it has focus when the window is asked for. */
function opener(): HTMLButtonElement {
  const button = document.createElement("button");
  button.textContent = "Open";
  document.body.appendChild(button);
  button.focus();
  return button;
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("LazyDialog", () => {
  it("shows the window once its chunk has loaded", async () => {
    const Loaded = lazy(async () => ({
      default: (() => <div data-testid="window">open</div>) as Component,
    }));
    render(() => (
      <LazyDialog name="Loaded">
        <Loaded />
      </LazyDialog>
    ));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect((await screen.findByTestId("window")).textContent).toBe("open");
  });

  it("keeps the page: an outer Suspense never falls back while the window loads", async () => {
    let arrive: (m: { default: Component }) => void = () => {};
    const Slow = lazy(() => new Promise<{ default: Component }>((r) => (arrive = r)));
    render(() => (
      <Suspense fallback={<div data-testid="page-gone" />}>
        <div data-testid="page" />
        <LazyDialog name="Slow">
          <Slow />
        </LazyDialog>
      </Suspense>
    ));
    await Promise.resolve();
    expect(screen.getByTestId("page")).toBeTruthy();
    expect(screen.queryByTestId("page-gone")).toBeNull();
    arrive({ default: () => <div data-testid="window">open</div> });
    expect((await screen.findByTestId("window")).textContent).toBe("open");
    expect(screen.queryByTestId("page-gone")).toBeNull();
  });

  describe("when the window's code cannot be loaded", () => {
    it("says so in a small dialog with Try again, Reload and Close, whatever the browser called the error", async () => {
      quiet();
      const Broken = brokenWindow();
      const { container } = render(() => (
        <LazyDialog name="Broken" onClose={() => {}}>
          <Broken />
        </LazyDialog>
      ));
      const dialog = await screen.findByRole("alertdialog");
      expect(dialog.getAttribute("aria-modal")).toBe("true");
      expect(dialog.textContent).toContain("could not be loaded");
      expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
      expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
      expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
      // On the page, not inside whatever holds the window (a drawer panel).
      expect(container.contains(dialog)).toBe(false);
      await tick();
      expect(dialog.contains(document.activeElement)).toBe(true);
    });

    it("closes the window on Close, gives focus back, and shows again on the next open", async () => {
      quiet();
      const Broken = brokenWindow();
      const button = opener();
      const [open, setOpen] = createSignal(true);
      render(() => (
        <Show when={open()}>
          <LazyDialog name="Broken" onClose={() => setOpen(false)}>
            <Broken />
          </LazyDialog>
        </Show>
      ));
      await screen.findByRole("alertdialog");
      await tick();
      fireEvent.click(screen.getByRole("button", { name: "Close" }));
      expect(screen.queryByRole("alertdialog")).toBeNull();
      expect(document.activeElement).toBe(button);

      // The same click that opened the window before must not do nothing now.
      setOpen(true);
      expect((await screen.findByRole("alertdialog")).textContent).toContain("could not be loaded");
    });

    it("closes on Escape like on Close", async () => {
      quiet();
      const Broken = brokenWindow();
      const [open, setOpen] = createSignal(true);
      render(() => (
        <Show when={open()}>
          <LazyDialog name="Broken" onClose={() => setOpen(false)}>
            <Broken />
          </LazyDialog>
        </Show>
      ));
      await screen.findByRole("alertdialog");
      fireEvent.keyDown(document.body, { key: "Escape" });
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });

    it("loads the window on Try again once the connection is back", async () => {
      quiet();
      let calls = 0;
      const Flaky = lazyWindow<Component>(() =>
        ++calls === 1
          ? Promise.reject(new TypeError("Load failed"))
          : Promise.resolve({ default: () => <div data-testid="window">open</div> }),
      );
      render(() => (
        <LazyDialog name="Flaky" onClose={() => {}}>
          <Flaky />
        </LazyDialog>
      ));
      await screen.findByRole("alertdialog");
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      expect((await screen.findByTestId("window")).textContent).toBe("open");
      expect(screen.queryByRole("alertdialog")).toBeNull();
      expect(calls).toBe(2);
    });

    it("cannot be dismissed into nothing when the window has no close (startup wizards), and can be retried in place", async () => {
      quiet();
      let calls = 0;
      const Wizard = lazyWindow<Component>(() =>
        ++calls === 1
          ? Promise.reject(new TypeError("Load failed"))
          : Promise.resolve({ default: () => <div data-testid="wizard">setup</div> }),
      );
      render(() => (
        <LazyDialog name="SetupWizard">
          <Wizard />
        </LazyDialog>
      ));
      const dialog = await screen.findByRole("alertdialog");
      expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
      fireEvent.keyDown(document.body, { key: "Escape" });
      expect(dialog.isConnected).toBe(true);
      // No forced reload: a reload during setup would lose its place.
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      expect((await screen.findByTestId("wizard")).textContent).toBe("setup");
    });

    it("gives focus back to what had focus when the window was asked for, not where it moved meanwhile", async () => {
      quiet();
      const button = opener();
      let fail: (e: unknown) => void = () => {};
      const Broken = lazyWindow(() => new Promise<{ default: Component }>((_, r) => (fail = r)));
      const [open, setOpen] = createSignal(true);
      render(() => (
        <Show when={open()}>
          <LazyDialog name="Broken" onClose={() => setOpen(false)}>
            <Broken />
          </LazyDialog>
        </Show>
      ));
      // Slow download: the user tabs on to something else.
      const other = document.createElement("button");
      document.body.appendChild(other);
      other.focus();
      fail(new TypeError("Load failed"));
      await screen.findByRole("alertdialog");
      await tick();
      fireEvent.click(screen.getByRole("button", { name: "Close" }));
      expect(document.activeElement).toBe(button);
    });

    it("opens in front of a window that is open and holds the keyboard", async () => {
      quiet();
      // For example quick actions (Ctrl+K), above the dialog layer.
      render(() => (
        <Modal onClose={() => {}} label="Quick actions" layer={MODAL_LAYER.dialog + 30}>
          <button type="button">inside</button>
        </Modal>
      ));
      const earlier = screen.getByRole("dialog", { name: "Quick actions" });
      const Broken = brokenWindow();
      render(() => (
        <LazyDialog name="Broken" onClose={() => {}}>
          <Broken />
        </LazyDialog>
      ));
      const dialog = await screen.findByRole("alertdialog");
      expect(zOf(dialog)).toBeGreaterThan(zOf(earlier));
      await tick();
      expect(dialog.contains(document.activeElement)).toBe(true);
    });
  });

  describe("when the window fails while rendering", () => {
    it("says the window had a problem, not the connection, and Try again renders it again", async () => {
      quiet();
      let fail = true;
      const Flaky: Component = () => {
        if (fail) throw new TypeError("Cannot read properties of undefined (reading 'id')");
        return <div data-testid="window">open</div>;
      };
      render(() => (
        <LazyDialog name="Flaky" onClose={() => {}}>
          <Flaky />
        </LazyDialog>
      ));
      const dialog = await screen.findByRole("alertdialog");
      expect(dialog.textContent).toContain("This window ran into a problem");
      expect(dialog.textContent).not.toContain("could not be loaded");
      expect(screen.queryByRole("button", { name: "Reload" })).toBeNull();
      expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();

      fail = false;
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      expect((await screen.findByTestId("window")).textContent).toBe("open");
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });

    it("offers only Try again for a startup wizard", async () => {
      quiet();
      const Throws: Component = () => {
        throw new Error("boom");
      };
      render(() => (
        <LazyDialog name="OnboardingWizard">
          <Throws />
        </LazyDialog>
      ));
      await screen.findByRole("alertdialog");
      expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Reload" })).toBeNull();
    });

    it("closes the window and its notice on Close", async () => {
      quiet();
      const Throws: Component = () => {
        throw new Error("boom");
      };
      const [open, setOpen] = createSignal(true);
      render(() => (
        <Show when={open()}>
          <LazyDialog name="Throws" onClose={() => setOpen(false)}>
            <Throws />
          </LazyDialog>
        </Show>
      ));
      await screen.findByRole("alertdialog");
      fireEvent.click(screen.getByRole("button", { name: "Close" }));
      expect(screen.queryByRole("alertdialog")).toBeNull();
      setOpen(true);
      expect(await screen.findByRole("alertdialog")).toBeTruthy();
    });
  });

  describe("while the window's code downloads", () => {
    const loadingText = () => screen.queryByRole("status")?.textContent ?? "";

    it("shows a polite loading sign from the delay on, and removes it when the window appears", async () => {
      vi.useFakeTimers();
      let arrive: (m: { default: Component }) => void = () => {};
      const Slow = lazy(() => new Promise<{ default: Component }>((r) => (arrive = r)));
      const { container } = render(() => (
        <LazyDialog name="Slow">
          <Slow />
        </LazyDialog>
      ));
      await vi.advanceTimersByTimeAsync(LOADING_DELAY_MS - 1);
      expect(loadingText()).toBe("");

      await vi.advanceTimersByTimeAsync(1);
      const status = screen.getByRole("status");
      expect(status.getAttribute("aria-live")).toBe("polite");
      expect(status.textContent).toMatch(/Loading/);
      expect(status.className).toContain("pointer-events-none");
      expect(container.contains(status)).toBe(false);
      expect(status.querySelector("button, [tabindex]")).toBeNull();

      arrive({ default: () => <div data-testid="window">open</div> });
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByTestId("window").textContent).toBe("open");
      expect(loadingText()).toBe("");
    });

    it("waits 350 ms", () => {
      expect(LOADING_DELAY_MS).toBe(350);
    });

    it("shows nothing at all when the code arrives quickly", async () => {
      vi.useFakeTimers();
      let arrive: (m: { default: Component }) => void = () => {};
      const Quick = lazy(() => new Promise<{ default: Component }>((r) => (arrive = r)));
      render(() => (
        <LazyDialog name="Quick">
          <Quick />
        </LazyDialog>
      ));
      await vi.advanceTimersByTimeAsync(LOADING_DELAY_MS - 1);
      expect(screen.queryByTestId("window")).toBeNull();
      expect(loadingText()).toBe("");
      arrive({ default: () => <div data-testid="window">open</div> });
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.getByTestId("window")).toBeTruthy();
      expect(loadingText()).toBe("");
    });

    it("shows one loading sign for several windows loading at once", async () => {
      vi.useFakeTimers();
      const arrivals: ((m: { default: Component }) => void)[] = [];
      const slow = () =>
        lazy(() => new Promise<{ default: Component }>((r) => arrivals.push(r)));
      const One = slow();
      const Two = slow();
      render(() => (
        <>
          <LazyDialog name="One">
            <One />
          </LazyDialog>
          <LazyDialog name="Two">
            <Two />
          </LazyDialog>
        </>
      ));
      await vi.advanceTimersByTimeAsync(LOADING_DELAY_MS - 1);
      expect(screen.queryAllByRole("status").every((s) => s.textContent === "")).toBe(true);
      await vi.advanceTimersByTimeAsync(1);
      expect(screen.getAllByRole("status")).toHaveLength(1);
      expect(loadingText()).toMatch(/Loading/);

      arrivals[0]({ default: () => <div data-testid="one">one</div> });
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByTestId("one")).toBeTruthy();
      expect(screen.getAllByRole("status")).toHaveLength(1);
      expect(loadingText()).toMatch(/Loading/);

      arrivals[1]({ default: () => <div data-testid="two">two</div> });
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.queryAllByRole("status")).toHaveLength(0);
    });

    it("replaces the loading sign with the notice when the download fails", async () => {
      vi.useFakeTimers();
      quiet();
      let fail: (e: unknown) => void = () => {};
      const Broken = lazyWindow(() => new Promise<{ default: Component }>((_, r) => (fail = r)));
      render(() => (
        <LazyDialog name="Broken" onClose={() => {}}>
          <Broken />
        </LazyDialog>
      ));
      await vi.advanceTimersByTimeAsync(LOADING_DELAY_MS);
      expect(loadingText()).toMatch(/Loading/);
      fail(new TypeError("Load failed"));
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByRole("alertdialog").textContent).toContain("could not be loaded");
      expect(screen.queryByRole("status")).toBeNull();
    });
  });
});
