import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { Show, Suspense, createSignal, lazy, type Component } from "solid-js";
import LazyDialog from "../LazyDialog";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

/** How browsers word a chunk that could not be downloaded (Chromium). */
const importFailed = () =>
  new TypeError("Failed to fetch dynamically imported module: /assets/Broken.js");

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
    expect(screen.queryByRole("alert")).toBeNull();
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

  it("says so with a reload when the chunk cannot load", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Broken = lazy<Component>(() => Promise.reject(importFailed()));
    render(() => (
      <LazyDialog name="Broken">
        <Broken />
      </LazyDialog>
    ));
    const notice = await screen.findByRole("alert");
    expect(notice.textContent).toContain("Reload");
    expect(screen.queryByTestId("window")).toBeNull();
  });

  it("lets the load-failure notice be closed, and shows it again on the next open", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Broken = lazy<Component>(() => Promise.reject(importFailed()));
    const [open, setOpen] = createSignal(true);
    const onClose = vi.fn(() => setOpen(false));
    render(() => (
      <Show when={open()}>
        <LazyDialog name="Broken" onClose={onClose}>
          <Broken />
        </LazyDialog>
      </Show>
    ));
    const notice = await screen.findByRole("alert");
    expect(notice.textContent).toContain("could not be loaded");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alert")).toBeNull();

    // The same click that opened the window before must not do nothing now.
    setOpen(true);
    expect((await screen.findByRole("alert")).textContent).toContain("could not be loaded");
  });

  it("says the window itself failed, not the connection, when it throws while rendering", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let fail = true;
    const Flaky: Component = () => {
      if (fail) throw new TypeError("Cannot read properties of undefined (reading 'id')");
      return <div data-testid="window">open</div>;
    };
    const onClose = vi.fn();
    render(() => (
      <LazyDialog name="Flaky" onClose={onClose}>
        <Flaky />
      </LazyDialog>
    ));
    const notice = await screen.findByRole("alert");
    expect(notice.textContent).toContain("This window ran into a problem");
    expect(notice.textContent).not.toContain("could not be loaded");
    expect(screen.queryByRole("button", { name: "Reload" })).toBeNull();

    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect((await screen.findByTestId("window")).textContent).toBe("open");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes the window when its error notice is closed", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Throws: Component = () => {
      throw new Error("boom");
    };
    const onClose = vi.fn();
    render(() => (
      <LazyDialog name="Throws" onClose={onClose}>
        <Throws />
      </LazyDialog>
    ));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("hides a closed notice even when the caller has no close handler", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Broken = lazy<Component>(() => Promise.reject(importFailed()));
    render(() => (
      <LazyDialog name="Broken">
        <Broken />
      </LazyDialog>
    ));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  describe("while the window's code downloads", () => {
    const loadingText = () => screen.queryByRole("status")?.textContent ?? "";

    it("shows a polite loading sign once it takes a moment, and removes it when the window appears", async () => {
      vi.useFakeTimers();
      let arrive: (m: { default: Component }) => void = () => {};
      const Slow = lazy(() => new Promise<{ default: Component }>((r) => (arrive = r)));
      render(() => (
        <LazyDialog name="Slow">
          <Slow />
        </LazyDialog>
      ));
      await vi.advanceTimersByTimeAsync(200);
      expect(loadingText()).toBe("");

      await vi.advanceTimersByTimeAsync(300);
      const status = screen.getByRole("status");
      expect(status.getAttribute("aria-live")).toBe("polite");
      expect(status.textContent).toMatch(/Loading/);

      arrive({ default: () => <div data-testid="window">open</div> });
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByTestId("window").textContent).toBe("open");
      expect(loadingText()).toBe("");
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
      await vi.advanceTimersByTimeAsync(100);
      arrive({ default: () => <div data-testid="window">open</div> });
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.getByTestId("window")).toBeTruthy();
      expect(loadingText()).toBe("");
    });

    it("replaces the loading sign with the notice when the download fails", async () => {
      vi.useFakeTimers();
      vi.spyOn(console, "error").mockImplementation(() => {});
      let fail: (e: unknown) => void = () => {};
      const Broken = lazy(() => new Promise<{ default: Component }>((_, r) => (fail = r)));
      render(() => (
        <LazyDialog name="Broken">
          <Broken />
        </LazyDialog>
      ));
      await vi.advanceTimersByTimeAsync(500);
      expect(loadingText()).toMatch(/Loading/);
      fail(importFailed());
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByRole("alert").textContent).toContain("could not be loaded");
      expect(loadingText()).toBe("");
    });
  });
});
