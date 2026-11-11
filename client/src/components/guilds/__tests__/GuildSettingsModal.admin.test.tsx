/**
 * ADMIN-5: leave (member) and delete (owner) the server, rename it, each
 * after a question. ADMIN-17: closing with unsaved changes asks first.
 * ADMIN-22: advanced tabs sit under their own heading.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const m = vi.hoisted(() => ({
  owner: false,
  leaveGuild: vi.fn(),
  deleteGuild: vi.fn(),
  updateGuild: vi.fn(),
  toast: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({ showToast: m.toast }));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));
vi.mock("@/stores/guilds", () => ({
  guildsState: { guilds: [{ id: "g1", name: "Familie", icon_url: null, owner_id: "x" }] },
  isGuildOwner: () => m.owner,
  leaveGuild: m.leaveGuild,
  deleteGuild: m.deleteGuild,
  updateGuild: m.updateGuild,
}));
vi.mock("@/stores/channels", () => ({
  channelsState: { selectedChannelId: null },
  clearSelection: vi.fn(),
  refreshChannelsForGuild: vi.fn(),
}));
vi.mock("@/stores/permissions", () => ({ memberHasPermission: () => m.owner }));
// Tabs are covered elsewhere; here they only report unsaved edits.
vi.mock("../GeneralTab", async () => ({ default: (await import("../ServerNameSection")).default }));
vi.mock("../MembersTab", () => ({ default: () => <div data-testid="members-tab" /> }));
vi.mock("../SafetyTab", () => ({
  default: (p: { onDirtyChange?: (d: boolean) => void }) => (
    <button onClick={() => p.onDirtyChange?.(true)}>make a change</button>
  ),
}));

import GuildSettingsModal from "../GuildSettingsModal";

beforeEach(() => {
  vi.clearAllMocks();
  m.owner = false;
});
afterEach(cleanup);

describe("GuildSettingsModal", () => {
  it("lets a member leave the server after a question", async () => {
    m.leaveGuild.mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(() => <GuildSettingsModal guildId="g1" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("guild-settings-exit"));
    expect(m.leaveGuild).not.toHaveBeenCalled();
    const dialog = screen.getByTestId("guild-settings-exit-dialog");
    expect(dialog.textContent).toContain("Leave Familie?");
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Leave server")!);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(m.leaveGuild).toHaveBeenCalledWith("g1");
  });

  it("lets the owner delete the server only after typing its name, and says why it failed", async () => {
    m.owner = true;
    m.deleteGuild.mockRejectedValue(new TypeError("Failed to fetch"));
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    fireEvent.click(screen.getByTestId("guild-settings-exit"));
    const dialog = screen.getByTestId("guild-settings-exit-dialog");
    const confirm = Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Delete server")!;
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    fireEvent.input(dialog.querySelector("input")!, { target: { value: "Familie" } });
    expect((confirm as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(confirm);
    expect((await screen.findByRole("alert")).textContent).toContain("Can't reach the server");
  });

  it("lets the owner rename the server", async () => {
    m.owner = true;
    m.updateGuild.mockResolvedValue({});
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    fireEvent.input(screen.getByLabelText("Server name"), { target: { value: "Familie Wolf" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(m.updateGuild).toHaveBeenCalledWith("g1", "Familie Wolf"));
    await waitFor(() => expect(m.toast).toHaveBeenCalledWith(expect.objectContaining({ title: "Server renamed to Familie Wolf" })));
  });

  it("asks before closing with unsaved changes", async () => {
    m.owner = true;
    const onClose = vi.fn();
    render(() => <GuildSettingsModal guildId="g1" onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: "Word filter" }));
    fireEvent.click(screen.getByRole("button", { name: "make a change" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Close" })[0]);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("guild-settings-unsaved")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("puts expert tabs under Advanced", () => {
    m.owner = true;
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    const nav = screen.getByRole("navigation");
    const text = nav.textContent ?? "";
    expect(text.indexOf("Advanced")).toBeGreaterThan(text.indexOf("Roles"));
    expect(text.indexOf("Webhooks")).toBeGreaterThan(text.indexOf("Advanced"));
  });
});
