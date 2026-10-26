import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({ secureCopy: vi.fn().mockResolvedValue({}) }));

vi.mock("@/lib/clipboard", () => ({ secureCopy: mocks.secureCopy }));
vi.mock("@/stores/guilds", () => ({
  guildsState: { isInvitesLoading: false },
  loadGuildInvites: vi.fn(),
  createInvite: vi.fn(),
  deleteInvite: vi.fn(),
  getGuildInvites: () => [
    { code: "AbCd1234", expires_at: null, use_count: 0, guild_id: "g1", id: "i1" },
  ],
}));

import InvitesTab from "../InvitesTab";

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
});
