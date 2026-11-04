// Codex r3 / Fable r2 R6: the app does not render with blank texts when an
// on-demand English area could not load; it says so and offers a reload.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";
import { StartupGate } from "../ChunkLoadNotice";

afterEach(cleanup);

describe("StartupGate", () => {
  it("blocks the app and offers a reload when the texts did not load", () => {
    const reload = vi.fn();
    render(() => (
      <StartupGate ready={false} onReload={reload}>
        <p>the app</p>
      </StartupGate>
    ));
    expect(screen.queryByText("the app")).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("could not be loaded");
    screen.getByRole("button", { name: "Reload" }).click();
    expect(reload).toHaveBeenCalledOnce();
  });

  it("renders the app once they did", () => {
    render(() => (
      <StartupGate ready={true}>
        <p>the app</p>
      </StartupGate>
    ));
    expect(screen.getByText("the app")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
