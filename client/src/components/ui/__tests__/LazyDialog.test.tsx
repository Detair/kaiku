import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import { Suspense, lazy, type Component } from "solid-js";
import LazyDialog from "../LazyDialog";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

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
    const Broken = lazy<Component>(() => Promise.reject(new Error("chunk gone")));
    render(() => (
      <LazyDialog name="Broken">
        <Broken />
      </LazyDialog>
    ));
    const notice = await screen.findByRole("alert");
    expect(notice.textContent).toContain("Reload");
    expect(screen.queryByTestId("window")).toBeNull();
  });
});
