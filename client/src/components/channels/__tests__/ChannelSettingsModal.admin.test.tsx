/** ADMIN-5: rename and delete a channel from its settings. */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const m = vi.hoisted(() => ({
  renameChannel: vi.fn(),
  deleteChannel: vi.fn(),
  refresh: vi.fn(),
  toast: vi.fn(),
}));
vi.mock("@/lib/tauri/channelAdmin", () => ({ renameChannel: m.renameChannel, deleteChannel: m.deleteChannel }));
vi.mock("@/components/ui/Toast", () => ({ showToast: m.toast }));
vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "c1", name: "allgemein", channel_type: "text" }], selectedChannelId: "c1" },
  clearSelection: vi.fn(),
  refreshChannelsForGuild: m.refresh,
  patchChannel: vi.fn(),
  setChannelsState: vi.fn(),
}));
vi.mock("@/stores/guilds", () => ({
  isGuildOwner: () => true,
  deleteGuild: vi.fn(),
  leaveGuild: vi.fn(),
  updateGuild: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));
vi.mock("@/stores/permissions", () => ({ memberHasPermission: () => true }));
vi.mock("@/stores/sound", () => ({
  getChannelNotificationLevel: () => "all",
  setChannelNotificationLevel: vi.fn(),
}));
vi.mock("../ChannelPermissions", () => ({ default: () => null }));

import ChannelSettingsModal from "../ChannelSettingsModal";

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("ChannelSettingsModal", () => {
  it("renames the channel and says so", async () => {
    m.renameChannel.mockResolvedValue({});
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={vi.fn()} />);
    fireEvent.input(screen.getByLabelText("Channel name"), { target: { value: "familie" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(m.renameChannel).toHaveBeenCalledWith("c1", "familie"));
    expect(m.refresh).toHaveBeenCalledWith("g1");
    await waitFor(() => expect(m.toast).toHaveBeenCalledWith(expect.objectContaining({ title: "Channel renamed to familie" })));
  });

  it("deletes the channel only after a question that names it", async () => {
    m.deleteChannel.mockResolvedValue(undefined);
    // The list refresh afterwards fails: the delete still counts (K7).
    m.refresh.mockRejectedValue(new TypeError("Failed to fetch"));
    const onClose = vi.fn();
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete channel" }));
    expect(m.deleteChannel).not.toHaveBeenCalled();
    const dialog = screen.getByTestId("channel-delete-dialog");
    expect(dialog.textContent).toContain("Delete the channel allgemein?");
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Delete channel")!);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(m.deleteChannel).toHaveBeenCalledWith("c1");
    expect(m.toast).toHaveBeenCalledWith(expect.objectContaining({ type: "success", title: "Channel allgemein deleted" }));
  });
});
