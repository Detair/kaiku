/**
 * K8: channel rename/delete is offered by the same rule the server uses:
 * guild permissions plus this channel's overrides.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

type Override = { role_id: string; allow_permissions: number; deny_permissions: number };
// null = the override list has not been loaded (or the server refused it).
const p = vi.hoisted(() => ({ overrides: [] as Override[] | null }));

vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "c1", name: "allgemein", channel_type: "text" }], selectedChannelId: null },
  clearSelection: vi.fn(),
  refreshChannelsForGuild: vi.fn(),
  patchChannel: vi.fn(),
  setChannelsState: vi.fn(),
}));
vi.mock("@/stores/guilds", () => ({ isGuildOwner: () => false, deleteGuild: vi.fn(), leaveGuild: vi.fn(), updateGuild: vi.fn() }));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));
vi.mock("@/stores/sound", () => ({ getChannelNotificationLevel: () => "all", setChannelNotificationLevel: vi.fn() }));
vi.mock("../ChannelPermissions", () => ({ default: () => null }));
vi.mock("@/stores/permissions", async () => {
  const { PermissionBits } = await import("@/lib/permissionConstants");
  return {
    getGuildRoles: () => [
      { id: "everyone", is_default: true, permissions: 0 },
      { id: "helper", is_default: false, permissions: PermissionBits.MANAGE_CHANNELS },
    ],
    getMemberRoleIds: () => ["helper"],
    getChannelOverrides: () => p.overrides ?? [],
    getLoadedChannelOverrides: () => p.overrides,
    loadChannelOverrides: vi.fn(),
    memberHasPermission: () => true,
  };
});

import ChannelSettingsModal from "../ChannelSettingsModal";
import { PermissionBits } from "@/lib/permissionConstants";

afterEach(cleanup);

describe("ChannelSettingsModal channel permission", () => {
  it("hides rename and delete when this channel denies managing it", () => {
    p.overrides = [{ role_id: "helper", allow_permissions: 0, deny_permissions: PermissionBits.MANAGE_CHANNELS }];
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Delete channel" })).toBeNull();
    expect(screen.queryByLabelText("Channel name")?.tagName).not.toBe("INPUT");
  });

  it("hides them while the channel's overrides are not loaded", () => {
    p.overrides = null;
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Delete channel" })).toBeNull();
  });

  it("shows them when nothing on this channel denies it", () => {
    p.overrides = [];
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Delete channel" })).toBeTruthy();
  });
});
