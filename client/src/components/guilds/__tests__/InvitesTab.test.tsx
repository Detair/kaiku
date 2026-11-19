import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  secureCopy: vi.fn().mockResolvedValue({}),
  createInvite: vi.fn(),
  deleteInvite: vi.fn(),
  toast: vi.fn(),
}));

vi.mock("@/lib/clipboard", () => ({ secureCopy: mocks.secureCopy }));
vi.mock("@/components/ui/Toast", () => ({ showToast: mocks.toast }));
vi.mock("@/stores/guilds", () => ({
  guildsState: { isInvitesLoading: false },
  loadGuildInvites: vi.fn(),
  createInvite: mocks.createInvite,
  deleteInvite: mocks.deleteInvite,
  getGuildInvites: () => [
    { code: "AbCd1234", expires_at: null, use_count: 0, guild_id: "g1", id: "i1" },
  ],
}));

import InvitesTab from "../InvitesTab";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("InvitesTab", () => {
  it("copies the invite code, not a link", async () => {
    render(() => <InvitesTab guildId="g1" />);
    fireEvent.click(screen.getByTitle("Copy invite code"));
    await waitFor(() => expect(mocks.secureCopy).toHaveBeenCalled());
    expect(mocks.secureCopy.mock.calls[0][0]).toBe("AbCd1234");
    expect(screen.getByTestId("invite-code").textContent).toBe("AbCd1234");
  });

  it("shows a new code large, copies it and says how to pass it on", async () => {
    mocks.createInvite.mockResolvedValue({ code: "NEWCODE1" });
    render(() => <InvitesTab guildId="g1" />);
    fireEvent.click(screen.getByTestId("create-invite-button"));
    const box = await screen.findByTestId("new-invite");
    expect(box.textContent).toContain("NEWCODE1");
    expect(box.textContent).toContain("Join Server");
    await waitFor(() => expect(mocks.secureCopy).toHaveBeenCalledWith("NEWCODE1", "invite_link"));
  });

  it("says why creating a code failed", async () => {
    mocks.createInvite.mockRejectedValue(new HttpError(403, "nope", "FORBIDDEN"));
    render(() => <InvitesTab guildId="g1" />);
    fireEvent.click(screen.getByTestId("create-invite-button"));
    await waitFor(() => expect(mocks.toast).toHaveBeenCalled());
    expect(mocks.toast.mock.calls[0][0]).toMatchObject({
      type: "error",
      title: "Could not create the invite code",
      message: "You are not allowed to do this here. Ask the owner of the server.",
    });
  });

  it("asks before deleting and keeps the dialog open with the reason on failure", async () => {
    mocks.deleteInvite.mockRejectedValue(new TypeError("Failed to fetch"));
    render(() => <InvitesTab guildId="g1" />);
    fireEvent.click(screen.getByRole("button", { name: "Delete invite code AbCd1234" }));
    expect(mocks.deleteInvite).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Delete code" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Can't reach the server");
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});
