import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({ adminCreateUser: vi.fn() }));

vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return {
    HttpError: common.HttpError,
    adminCreateUser: mocks.adminCreateUser,
    getServerUrl: () => "https://kaiku.example",
  };
});
vi.mock("@/stores/admin", () => ({
  adminState: { isElevated: true },
  createUser: (req: unknown) => mocks.adminCreateUser(req),
}));

import CreateAccountModal from "../CreateAccountModal";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("CreateAccountModal", () => {
  it("create account shows the username-taken sentence", async () => {
    mocks.adminCreateUser.mockRejectedValue(new HttpError(409, "x", "USER_EXISTS"));
    const { container } = render(() => <CreateAccountModal onClose={vi.fn()} />);
    const input = container.querySelector("input") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "anna" } });
    fireEvent.submit(container.querySelector("form")!);
    expect(await screen.findByText("That username is already taken.")).toBeTruthy();
  });
});
