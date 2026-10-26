import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const confirmElevation = vi.hoisted(() => vi.fn());
const reauthWithOidc = vi.hoisted(() => vi.fn());
const loadInfo = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", async () => ({
  // The real class, so the shared errorText() recognises it.
  HttpError: (await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common"))
    .HttpError,
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/admin", () => ({
  confirmElevation: (...a: unknown[]) => confirmElevation(...a),
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
    confirmElevation.mockReset();
    reauthWithOidc.mockReset().mockResolvedValue(undefined);
    loadInfo.mockReset().mockResolvedValue({
      methods: [{ kind: "oidc", providerId: "kc", label: "Keycloak" }],
      mfaRequired: false,
    });
  });

  const click = async () =>
    fireEvent.click(await screen.findByRole("button", { name: "Confirm with Keycloak" }));

  it("shows a loading state while the methods load", () => {
    loadInfo.mockReturnValue(new Promise(() => {}));
    render(() => <ElevateDialog onClose={() => {}} />);
    expect(screen.getByRole("status").textContent).toContain("Checking");
  });

  it("does not close until the server confirms elevation, and says so", async () => {
    confirmElevation.mockResolvedValue(false);
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} />);
    await click();
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("did not confirm"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("fails closed when the status call fails", async () => {
    confirmElevation.mockRejectedValue(new Error("network"));
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} />);
    await click();
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes once the server confirms elevation", async () => {
    confirmElevation.mockResolvedValue(true);
    const onClose = vi.fn();
    render(() => <ElevateDialog onClose={onClose} />);
    await click();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("runs no status check after the dialog was closed mid-flow", async () => {
    let finish: () => void = () => {};
    reauthWithOidc.mockReturnValue(new Promise<void>((r) => (finish = r)));
    const onClose = vi.fn();
    const { unmount } = render(() => <ElevateDialog onClose={onClose} />);
    await click();
    await waitFor(() => expect(reauthWithOidc).toHaveBeenCalled());
    unmount();
    finish();
    await new Promise((r) => setTimeout(r, 10));
    expect(confirmElevation).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
