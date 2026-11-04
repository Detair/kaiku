// Admin mode by step-up (F7, A5): a passkey (or the phone) confirms
// `admin.elevate`, then only the reason is posted. No password, no SSO.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";
import { createSignal, Show } from "solid-js";

const stepUp = vi.hoisted(() => vi.fn());
const elevateWithStepUp = vi.hoisted(() => vi.fn());
const methods = vi.hoisted(() => vi.fn());
const toast = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri/passkeys", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/passkeys")),
  stepUpWithPasskey: (...a: unknown[]) => stepUp(...a),
  canUsePasskeys: () => true,
}));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getStepUpMethods: methods }));
vi.mock("@/components/ui/Toast", () => ({ showToast: toast }));
vi.mock("@/stores/admin", () => ({
  elevateWithStepUp: (...a: unknown[]) => elevateWithStepUp(...a),
}));

import ElevateDialog from "../ElevateDialog";
import StepUpHost from "@/components/auth/StepUpDialog";
import { pendingStepUp } from "@/lib/stepUp";
import { setAuthState } from "@/stores/auth";
import { HttpError } from "@/lib/tauri/common";
import type { User } from "@/lib/types";

function fullSession() {
  setAuthState({
    user: { id: "u1", username: "a", session: { id: "s1", assurance: "full", auth_methods: ["passkey"] } } as unknown as User,
  });
}

beforeEach(() => {
  stepUp.mockReset().mockResolvedValue({ action: "admin.elevate", target: "", via: "passkey", expires_at: "x" });
  elevateWithStepUp.mockReset().mockResolvedValue({ elevated: true, expires_at: "2099-01-01T00:00:00Z" });
  methods.mockReset().mockResolvedValue({ passkey: true, device: true });
  toast.mockReset();
  fullSession();
});

afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("ElevateDialog", () => {
  it("elevation posts only reason after a passkey step-up", async () => {
    const onElevated = vi.fn();
    const onClose = vi.fn();
    render(() => (
      <>
        <StepUpHost />
        <ElevateDialog onClose={onClose} onElevated={onElevated} />
      </>
    ));
    expect(await screen.findByText("Turn on admin mode")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Confirm with a passkey" }));
    await waitFor(() => expect(onElevated).toHaveBeenCalledOnce());
    expect(stepUp).toHaveBeenCalledWith("admin.elevate", "", expect.any(AbortSignal));
    expect(elevateWithStepUp).toHaveBeenCalledWith(null);
    expect(stepUp.mock.invocationCallOrder[0]).toBeLessThan(elevateWithStepUp.mock.invocationCallOrder[0]);
    expect(onClose).toHaveBeenCalled();
  });

  it("password and SSO re-auth are no longer offered", async () => {
    methods.mockResolvedValue({ passkey: false, device: true });
    render(() => (
      <>
        <StepUpHost />
        <ElevateDialog onClose={vi.fn()} />
      </>
    ));
    expect(await screen.findByText("Confirm this in the Kaiku app on your phone.")).toBeTruthy();
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(document.querySelector('input[autocomplete="one-time-code"]')).toBeNull();
    expect(screen.queryByRole("button", { name: /Confirm with / })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(pendingStepUp()).toBeNull());
    expect(elevateWithStepUp).not.toHaveBeenCalled();
  });

  it("a non-admin is told that only a server admin can do this", async () => {
    stepUp.mockRejectedValue(new HttpError(403, "x", "NOT_ADMIN"));
    render(() => (
      <>
        <StepUpHost />
        <ElevateDialog onClose={vi.fn()} />
      </>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with a passkey" }));
    expect(await screen.findByText("Only a server admin can do this.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  });

  it("a refused elevation says why and closes", async () => {
    elevateWithStepUp.mockRejectedValue(new HttpError(403, "x", "NOT_ADMIN"));
    const onClose = vi.fn();
    render(() => (
      <>
        <StepUpHost />
        <ElevateDialog onClose={onClose} />
      </>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with a passkey" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ message: "Only a server admin can do this." }));
  });

  it("unmounting the dialog cancels the waiting step-up", async () => {
    const [shown, setShown] = createSignal(true);
    render(() => (
      <>
        <StepUpHost />
        <Show when={shown()}>
          <ElevateDialog onClose={() => setShown(false)} />
        </Show>
      </>
    ));
    await screen.findByText("Turn on admin mode");
    setShown(false);
    await waitFor(() => expect(pendingStepUp()).toBeNull());
    expect(stepUp).not.toHaveBeenCalled();
  });
});
