import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const updatePasswordMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  updatePassword: updatePasswordMock,
  revokeAllOtherSessions: vi.fn(),
}));

import ChangePasswordModal from "../ChangePasswordModal";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  updatePasswordMock.mockReset();
});

function submit(container: HTMLElement): void {
  const inputs = container.querySelectorAll<HTMLInputElement>('input[type="password"]');
  fireEvent.input(inputs[0], { target: { value: "oldpassword" } });
  fireEvent.input(inputs[1], { target: { value: "newpassword1" } });
  fireEvent.input(inputs[2], { target: { value: "newpassword1" } });
  fireEvent.submit(container.querySelector("form")!);
}

describe("ChangePasswordModal", () => {
  it("shows the wrong-current-password sentence for INVALID_CREDENTIALS", async () => {
    updatePasswordMock.mockRejectedValue(new HttpError(401, "x", "INVALID_CREDENTIALS"));
    const { container } = render(() => <ChangePasswordModal onClose={() => {}} />);
    submit(container);
    await waitFor(() => expect(screen.getByText("Your current password is not correct.")).toBeTruthy());
  });

  it("falls back to the generic sentence for other errors", async () => {
    updatePasswordMock.mockRejectedValue(new HttpError(400, "raw server text", "WHATEVER"));
    const { container } = render(() => <ChangePasswordModal onClose={() => {}} />);
    submit(container);
    await waitFor(() => expect(screen.getByText("Failed to update password")).toBeTruthy());
    expect(screen.queryByText(/raw server text/)).toBeNull();
  });
});
