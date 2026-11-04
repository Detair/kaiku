// Fable r2 R1, Flash part 3 #1: the chat-only banner and the open-request
// prompt are a lazy chunk; when it cannot load the page says so.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/components/security/SecurityHosts", () => {
  throw new Error("chunk load failed");
});

import SecurityHostsSlot from "../SecurityHostsSlot";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("security hosts whose chunk cannot load", () => {
  it("show a notice with a reload instead of nothing", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const reload = vi.fn();
    render(() => <SecurityHostsSlot onReload={reload} />);
    expect((await screen.findByRole("alert")).textContent).toContain("could not be loaded");
    screen.getByRole("button", { name: "Reload" }).click();
    expect(reload).toHaveBeenCalledOnce();
  });
});
