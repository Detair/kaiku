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
// A real store: deleting or leaving removes the server, as stores/guilds.ts does,
// so "who is the owner" changes after the call (W2).
const g = vi.hoisted(() => ({ state: null as unknown as { guilds: { id: string; name: string; icon_url: null; owner_id: string }[] }, set: null as unknown as (k: "guilds", v: unknown) => void }));
vi.mock("@/stores/guilds", async () => {
  const { createStore } = await import("solid-js/store");
  const [state, set] = createStore({ guilds: [] as { id: string; name: string; icon_url: null; owner_id: string }[] });
  g.state = state;
  g.set = set as never;
  const remove = (id: string) => set("guilds", (list) => list.filter((x) => x.id !== id));
  return {
    guildsState: state,
    isGuildOwner: (gid: string, uid: string) => state.guilds.find((x) => x.id === gid)?.owner_id === uid,
    leaveGuild: async (id: string) => {
      await m.leaveGuild(id);
      remove(id);
    },
    deleteGuild: async (id: string) => {
      await m.deleteGuild(id);
      remove(id);
    },
    updateGuild: m.updateGuild,
  };
});
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
  g.set("guilds", [{ id: "g1", name: "Familie", icon_url: null, owner_id: "x" }]);
});
const becomeOwner = () => {
  m.owner = true;
  g.set("guilds", [{ id: "g1", name: "Familie", icon_url: null, owner_id: "me" }]);
};
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
    becomeOwner();
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

  it("says the server was deleted, not left, after the owner deletes it (W2)", async () => {
    becomeOwner();
    m.deleteGuild.mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(() => <GuildSettingsModal guildId="g1" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("guild-settings-exit"));
    const dialog = screen.getByTestId("guild-settings-exit-dialog");
    fireEvent.input(dialog.querySelector("input")!, { target: { value: "Familie" } });
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Delete server")!);
    await waitFor(() => expect(m.toast).toHaveBeenCalled());
    expect(m.toast.mock.calls[0][0].title).toBe("The server Familie was deleted");
  });

  it("lets the owner rename the server", async () => {
    becomeOwner();
    m.updateGuild.mockResolvedValue({});
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    fireEvent.input(screen.getByLabelText("Server name"), { target: { value: "Familie Wolf" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(m.updateGuild).toHaveBeenCalledWith("g1", "Familie Wolf"));
    await waitFor(() => expect(m.toast).toHaveBeenCalledWith(expect.objectContaining({ title: "Server renamed to Familie Wolf" })));
  });

  it("asks before closing with unsaved changes", async () => {
    becomeOwner();
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

  it("asks before switching tabs with unsaved changes, and switches only after discarding (W3)", async () => {
    becomeOwner();
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Word filter" }));
    fireEvent.click(screen.getByRole("button", { name: "make a change" }));
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    expect(screen.getByTestId("guild-settings-unsaved")).toBeTruthy();
    expect(screen.queryByTestId("members-tab")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "make a change" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    await screen.findByTestId("members-tab");
  });

  it("protects a typed but unsaved server name (K1)", () => {
    becomeOwner();
    const onClose = vi.fn();
    render(() => <GuildSettingsModal guildId="g1" onClose={onClose} />);
    fireEvent.input(screen.getByLabelText("Server name"), { target: { value: "Neu" } });
    fireEvent.click(screen.getAllByRole("button", { name: "Close" })[0]);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("guild-settings-unsaved")).toBeTruthy();
  });

  it("puts expert tabs under Advanced", () => {
    becomeOwner();
    render(() => <GuildSettingsModal guildId="g1" onClose={vi.fn()} />);
    const nav = screen.getByRole("navigation");
    const text = nav.textContent ?? "";
    expect(text.indexOf("Advanced")).toBeGreaterThan(text.indexOf("Roles"));
    expect(text.indexOf("Webhooks")).toBeGreaterThan(text.indexOf("Advanced"));
  });
});
