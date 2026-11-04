// Fable r2 R1: the session dialogs are a lazy chunk. When it cannot load,
// the page says so with a reload, and a step-up fails instead of hanging.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/components/auth/SessionDialogHosts", () => {
  throw new Error("chunk load failed");
});
const methods = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getStepUpMethods: methods }));

import SessionDialogsSlot from "../SessionDialogsSlot";
import { StepUpUnavailable, pendingStepUp, withStepUp } from "@/lib/stepUp";
import { setAuthState } from "@/stores/auth";
import type { User } from "@/lib/types";

const req = { action: "key_device.remove" as const, target: "d1", targetLabel: "Pixel 8" };

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  methods.mockResolvedValue({ passkey: true, device: false });
  setAuthState({
    user: { id: "u1", username: "a", session: { id: "s1", assurance: "full" } } as unknown as User,
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  setAuthState({ user: null });
});

describe("session dialogs whose chunk cannot load", () => {
  it("a step-up waiting for the dialog fails once the chunk fails, and the page offers a reload", async () => {
    const run = vi.fn();
    const asked = withStepUp(req, run);
    await vi.waitFor(() => expect(pendingStepUp()).not.toBeNull());
    render(() => <SessionDialogsSlot />);
    await expect(asked).rejects.toBeInstanceOf(StepUpUnavailable);
    expect(run).not.toHaveBeenCalled();
    expect((await screen.findByRole("alert")).textContent).toContain("could not be loaded");
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
  });

  it("a chat-only session gets an error, not a silent cancel, when the dialogs failed (Fable r2 follow-up b)", async () => {
    render(() => <SessionDialogsSlot />);
    await screen.findByRole("alert");
    setAuthState({
      user: { id: "u1", username: "a", session: { id: "s1", assurance: "probation" } } as unknown as User,
    });
    await expect(withStepUp(req, vi.fn())).rejects.toBeInstanceOf(StepUpUnavailable);
  });

  it("a later step-up fails at once instead of hanging", async () => {
    render(() => <SessionDialogsSlot />);
    await screen.findByRole("alert");
    await expect(withStepUp(req, vi.fn())).rejects.toBeInstanceOf(StepUpUnavailable);
    expect(pendingStepUp()).toBeNull();
  });
});
