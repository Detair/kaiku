import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const state = vi.hoisted(() => ({ adminState: { isElevated: false } }));
const checkAdminStatus = vi.hoisted(() => vi.fn());
const reauthWithOidc = vi.hoisted(() => vi.fn());
const loadInfo = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({ HttpError: class extends Error {} }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/admin", () => ({
  adminState: state.adminState,
  checkAdminStatus: (...a: unknown[]) => checkAdminStatus(...a),
  elevateSession: vi.fn(),
}));
vi.mock("@/lib/reauth", async (orig) => ({
  ...(await orig<object>()),
  defaultReauthStrategy: { loadInfo, reauthWithOidc },
}));

import ElevateDialog from "../ElevateDialog";

describe("ElevateDialog provider path", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    state.adminState.isElevated = false;
    checkAdminStatus.mockReset();
    reauthWithOidc.mockReset().mockResolvedValue(undefined);
    loadInfo.mockReset().mockResolvedValue({
      methods: [{ kind: "oidc", providerId: "kc", label: "Keycloak" }],
      mfaRequired: false,
    });
  });

  it("does not close until the server confirms elevation", async () => {
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} />);
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with Keycloak" }));
    await waitFor(() => expect(checkAdminStatus).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("closes once the status call reports elevation", async () => {
    checkAdminStatus.mockImplementation(async () => {
      state.adminState.isElevated = true;
    });
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} />);
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with Keycloak" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });
});
