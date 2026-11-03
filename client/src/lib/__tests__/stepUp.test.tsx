import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const tauriMode = vi.hoisted(() => ({ on: false }));
const methods = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getStepUpMethods: methods }));
const stepUp = vi.hoisted(() => vi.fn());
const usable = vi.hoisted(() => ({ on: true }));
vi.mock("@/lib/tauri/passkeys", async (orig) => ({
  ...(await orig<object>()),
  stepUpWithPasskey: stepUp,
  canUsePasskeys: () => usable.on,
}));
const fullSession = vi.hoisted(() => vi.fn());
vi.mock("@/components/auth/FullSessionRequiredDialog", async (orig) => ({
  ...(await orig<object>()),
  showFullSessionRequired: fullSession,
}));

import { StepUpCancelled, withStepUp, STEP_UP_LABELS } from "../stepUp";
import StepUpHost from "@/components/auth/StepUpDialog";
import { PasskeyError } from "../tauri/passkeys";
import { setAuthState } from "@/stores/auth";
import { HttpError } from "../tauri/common";
import { STEP_UP_ACTIONS, type User } from "../types";

const full = { id: "s1", assurance: "full", auth_methods: ["passkey"], auth_time: "", client_class: "web", absolute_expires_at: "", key_device_id: null };

function signIn(assurance: "full" | "probation") {
  setAuthState({ user: { id: "u1", username: "a", session: { ...full, assurance } } as unknown as User });
}

const grant = { action: "key_device.remove", target: "d1", via: "passkey", expires_at: "" };

beforeEach(() => {
  stepUp.mockReset();
  stepUp.mockResolvedValue(grant);
  usable.on = true;
  methods.mockResolvedValue({ passkey: true, device: false });
  fullSession.mockReset();
  tauriMode.on = false;
  signIn("full");
  render(() => <StepUpHost />);
});

afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

const req = { action: "key_device.remove" as const, target: "d1", targetLabel: "Pixel 8" };

async function confirm() {
  fireEvent.click(await screen.findByRole("button", { name: "Confirm with a passkey" }));
}

describe("withStepUp", () => {
  it("withStepUp runs the passkey ceremony then the action", async () => {
    const run = vi.fn().mockResolvedValue("done");
    const p = withStepUp(req, run);
    await confirm();
    await expect(p).resolves.toBe("done");
    expect(stepUp).toHaveBeenCalledWith("key_device.remove", "d1", expect.any(AbortSignal));
    expect(stepUp.mock.invocationCallOrder[0]).toBeLessThan(run.mock.invocationCallOrder[0]);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("dialog names the action and target label", async () => {
    const p = withStepUp(req, vi.fn());
    expect(await screen.findByText("Remove the device Pixel 8")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(p).rejects.toBeInstanceOf(StepUpCancelled);
  });

  it("probation opens the full-session dialog and runs no ceremony", async () => {
    signIn("probation");
    const run = vi.fn();
    await expect(withStepUp(req, run)).rejects.toBeInstanceOf(StepUpCancelled);
    expect(fullSession).toHaveBeenCalled();
    expect(stepUp).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
  });

  it("no passkey shows the phone hint", async () => {
    methods.mockResolvedValue({ passkey: false, device: true });
    const run = vi.fn();
    const p = withStepUp(req, run);
    expect(await screen.findByText("Confirm this in the Kaiku app on your phone.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Confirm with a passkey" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(p).rejects.toBeInstanceOf(StepUpCancelled);
    expect(run).not.toHaveBeenCalled();
  });

  it("STEP_UP_REQUIRED retries once with a new grant", async () => {
    const run = vi
      .fn()
      .mockRejectedValueOnce(new HttpError(403, "x", "STEP_UP_REQUIRED"))
      .mockResolvedValueOnce("ok");
    const p = withStepUp(req, run);
    await confirm();
    await confirm();
    await expect(p).resolves.toBe("ok");
    expect(stepUp).toHaveBeenCalledTimes(2);
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("a second STEP_UP_REQUIRED is not retried again", async () => {
    const run = vi.fn().mockRejectedValue(new HttpError(403, "x", "STEP_UP_REQUIRED"));
    const p = withStepUp(req, run);
    await confirm();
    await confirm();
    await expect(p).rejects.toMatchObject({ code: "STEP_UP_REQUIRED" });
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("closing the dialog aborts and rejects StepUpCancelled", async () => {
    let signal!: AbortSignal;
    stepUp.mockImplementation((_a: string, _t: string, s: AbortSignal) => {
      signal = s;
      return new Promise(() => {});
    });
    const run = vi.fn();
    const p = withStepUp(req, run);
    await confirm();
    fireEvent.click(await screen.findByRole("button", { name: "Close" }));
    await expect(p).rejects.toBeInstanceOf(StepUpCancelled);
    expect(signal.aborted).toBe(true);
    expect(run).not.toHaveBeenCalled();
  });

  it("desktop says the browser continues and names the browser setting on failure", async () => {
    tauriMode.on = true;
    let fail!: (e: unknown) => void;
    stepUp.mockImplementation(() => new Promise((_r, reject) => (fail = reject)));
    const p = withStepUp(req, vi.fn());
    await confirm();
    expect(await screen.findByText("Continue in your browser.")).toBeTruthy();
    fail(new PasskeyError("PASSKEY_BROWSER_FAILED"));
    expect(
      await screen.findByText(
        "Kaiku could not open the browser chosen under Settings → Browser for passkey sign-in. Choose another one there.",
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(p).rejects.toBeInstanceOf(StepUpCancelled);
  });

  it("a client that cannot run a passkey ceremony shows the phone hint", async () => {
    usable.on = false;
    const p = withStepUp(req, vi.fn());
    expect(await screen.findByText("Confirm this in the Kaiku app on your phone.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Confirm with a passkey" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(p).rejects.toBeInstanceOf(StepUpCancelled);
    expect(stepUp).not.toHaveBeenCalled();
  });

  it("every action of the table has a label", () => {
    for (const a of STEP_UP_ACTIONS) expect(STEP_UP_LABELS[a]).toMatch(/^security\.stepUp\./);
  });
});
