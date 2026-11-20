/**
 * The channel override editor changes only the bits it shows: a save keeps
 * every other allow/deny bit that is stored for the role.
 */
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, within } from "@solidjs/testing-library";

type Override = { role_id: string; allow_permissions: number; deny_permissions: number };
const p = vi.hoisted(() => ({
  overrides: [] as Override[],
  setChannelOverride: vi.fn(),
}));

vi.mock("@/stores/permissions", () => ({
  loadChannelOverrides: vi.fn(),
  getChannelOverrides: () => p.overrides,
  setChannelOverride: p.setChannelOverride,
  deleteChannelOverride: vi.fn(),
  loadGuildRoles: vi.fn(),
  getGuildRoles: () => [
    { id: "everyone", name: "@everyone", is_default: true, permissions: 0, color: null },
  ],
  permissionsState: { isOverridesLoading: false },
}));
vi.mock("@/stores/serverFeatures", () => ({ isPermissionVisible: () => true }));
vi.mock("@/components/guilds/settingsAction", () => ({ reportFailure: vi.fn() }));

import ChannelPermissions from "../ChannelPermissions";
import { PermissionBits } from "@/lib/permissionConstants";

const HIGH = 2 ** 40;
// The server's SCREEN_SHARE bit (server/src/permissions/guild.rs).
const SCREEN_SHARE = 1 << 22;

afterEach(cleanup);
beforeEach(() => {
  p.setChannelOverride.mockReset();
  p.setChannelOverride.mockResolvedValue({});
});

function openEditorAndDenySending() {
  render(() => <ChannelPermissions channelId="c1" guildId="g1" />);
  fireEvent.click(screen.getByTestId("channel-permissions-override-edit"));
  const sendRow = screen.getByTestId("channel-permissions-row-send-messages");
  fireEvent.click(within(sendRow).getByTestId("channel-permissions-deny"));
  fireEvent.click(screen.getByTestId("channel-permissions-save"));
}

describe("ChannelPermissions save", () => {
  it("keeps a stored screen-share deny when another permission is changed", async () => {
    p.overrides = [
      { role_id: "everyone", allow_permissions: 0, deny_permissions: SCREEN_SHARE },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.deny).toBe(SCREEN_SHARE | PermissionBits.SEND_MESSAGES);
    expect(body.allow).toBe(0);
  });

  it("keeps every stored bit the editor does not list, also above bit 31", async () => {
    const unlistedAllow = PermissionBits.MANAGE_EVENTS + HIGH * 2;
    const unlistedDeny = PermissionBits.MANAGE_POSTS + PermissionBits.SEND_ANNOUNCEMENTS + HIGH;
    p.overrides = [
      {
        role_id: "everyone",
        allow_permissions: unlistedAllow + PermissionBits.SEND_MESSAGES,
        deny_permissions: unlistedDeny,
      },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.allow).toBe(unlistedAllow);
    expect(body.deny).toBe(unlistedDeny + PermissionBits.SEND_MESSAGES);
  });

  it("offers screen sharing as a channel override", () => {
    p.overrides = [{ role_id: "everyone", allow_permissions: 0, deny_permissions: 0 }];
    render(() => <ChannelPermissions channelId="c1" guildId="g1" />);
    fireEvent.click(screen.getByTestId("channel-permissions-override-edit"));
    expect(screen.getByTestId("channel-permissions-row-screen-share")).toBeTruthy();
  });
});
