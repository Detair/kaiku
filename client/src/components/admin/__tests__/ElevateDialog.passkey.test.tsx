import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const stepUp = vi.hoisted(() => vi.fn());
const elevateWithStepUp = vi.hoisted(() => vi.fn());
const confirmElevation = vi.hoisted(() => vi.fn());
const loadInfo = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", async () => ({
  HttpError: (await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common"))
    .HttpError,
}));
vi.mock("@/lib/tauri/passkeys", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/passkeys")),
  stepUpWithPasskey: (...a: unknown[]) => stepUp(...a),
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/admin", () => ({
  confirmElevation: (...a: unknown[]) => confirmElevation(...a),
  elevateSession: vi.fn(),
  elevateWithStepUp: (...a: unknown[]) => elevateWithStepUp(...a),
}));
vi.mock("@/lib/reauth", async (orig) => ({
  ...(await orig<object>()),
  defaultReauthStrategy: { loadInfo, reauthWithOidc: vi.fn() },
}));

import ElevateDialog from "../ElevateDialog";

describe("ElevateDialog passkey path", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    (globalThis as { PublicKeyCredential?: unknown }).PublicKeyCredential ??= {};
    if (!navigator.credentials) {
      Object.defineProperty(navigator, "credentials", { value: { get: () => undefined }, configurable: true });
    }
    stepUp.mockReset().mockResolvedValue({ action: "admin.elevate", target: "", via: "passkey", expires_at: "x" });
    elevateWithStepUp.mockReset().mockResolvedValue(undefined);
    confirmElevation.mockReset().mockResolvedValue(true);
    loadInfo.mockReset().mockResolvedValue({ methods: [{ kind: "passkey" }], mfaRequired: false });
  });

  it("passkey elevation calls step-up then elevate with reason only", async () => {
    const onElevated = vi.fn();
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} onElevated={onElevated} />);
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with a passkey" }));
    await waitFor(() => expect(onElevated).toHaveBeenCalledOnce());
    expect(stepUp).toHaveBeenCalledWith("admin.elevate", null, expect.any(AbortSignal));
    expect(elevateWithStepUp).toHaveBeenCalledWith(null);
    expect(stepUp.mock.invocationCallOrder[0]).toBeLessThan(elevateWithStepUp.mock.invocationCallOrder[0]);
    expect(onClose).toHaveBeenCalled();
  });
});
