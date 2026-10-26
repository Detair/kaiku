import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({ setElevated: (_v: boolean) => {} }));

vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, getServerUrl: () => "https://kaiku.example" };
});
vi.mock("@/stores/admin", async () => {
  const { createStore } = await import("solid-js/store");
  const [adminState, setAdminState] = createStore({ isElevated: false });
  mocks.setElevated = (v) => setAdminState("isElevated", v);
  return { adminState, createUser: vi.fn() };
});
vi.mock("@/stores/guilds", () => ({ guildsState: { guilds: [] } }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
// The real dialog is covered by ElevateDialog.test.tsx.
vi.mock("../ElevateDialog", () => ({
  default: (props: { onClose: () => void; onElevated?: () => void }) => (
    <div role="dialog" aria-label="Elevate">
      <button
        type="button"
        onClick={() => {
          mocks.setElevated(true);
          props.onElevated?.();
          props.onClose();
        }}
      >
        Confirm elevation
      </button>
    </div>
  ),
}));

import CreateAccountModal from "../CreateAccountModal";

afterEach(() => {
  cleanup();
  mocks.setElevated(false);
});

describe("CreateAccountModal unlock", () => {
  it("offers to unlock admin actions and enables the fields afterwards", async () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    const username = screen.getByLabelText(/^Username/) as HTMLInputElement;
    expect(username.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Unlock admin actions" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm elevation" }));

    expect((screen.getByLabelText(/^Username/) as HTMLInputElement).disabled).toBe(false);
    expect(screen.queryByRole("button", { name: "Unlock admin actions" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Elevate" })).toBeNull();
  });

  it("hides the unlock button when the session is already elevated", () => {
    mocks.setElevated(true);
    render(() => <CreateAccountModal onClose={() => {}} />);
    expect(screen.queryByRole("button", { name: "Unlock admin actions" })).toBeNull();
  });
});
