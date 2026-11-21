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
  hidden: new Set<string>(),
  roles: [] as { id: string; name: string; is_default: boolean; permissions: number; color: null }[],
}));
const EVERYONE = { id: "everyone", name: "@everyone", is_default: true, permissions: 0, color: null };
const MODS = { id: "mods", name: "Mods", is_default: false, permissions: 0, color: null };

vi.mock("@/stores/permissions", () => ({
  loadChannelOverrides: vi.fn(),
  getChannelOverrides: () => p.overrides,
  setChannelOverride: p.setChannelOverride,
  deleteChannelOverride: vi.fn(),
  loadGuildRoles: vi.fn(),
  getGuildRoles: () => p.roles,
  permissionsState: { isOverridesLoading: false },
}));
vi.mock("@/stores/serverFeatures", () => ({
  isPermissionVisible: (key: string) => !p.hidden.has(key),
}));
vi.mock("@/components/guilds/settingsAction", () => ({ reportFailure: vi.fn() }));

import ChannelPermissions from "../ChannelPermissions";
import { PermissionBits } from "@/lib/permissionConstants";

const HIGH = 2 ** 40;
// The server's SCREEN_SHARE bit (server/src/permissions/guild.rs).
const SCREEN_SHARE = 1 << 22;

afterEach(() => {
  cleanup();
  p.hidden.clear();
});
beforeEach(() => {
  p.roles = [EVERYONE];
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
  it("keeps a stored allow and deny of permissions it does not list", async () => {
    // Forum and event rights are not offered per channel; a deny and an
    // allow stored for them survive an unrelated change (of a role other
    // than @everyone; see below for @everyone).
    p.roles = [MODS];
    p.overrides = [
      {
        role_id: "mods",
        allow_permissions: PermissionBits.MANAGE_EVENTS,
        deny_permissions: PermissionBits.MANAGE_POSTS,
      },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.allow).toBe(PermissionBits.MANAGE_EVENTS);
    expect(body.deny).toBe(PermissionBits.MANAGE_POSTS + PermissionBits.SEND_MESSAGES);
  });

  it("round-trips a stored screen-share deny (a listed permission)", async () => {
    p.overrides = [
      { role_id: "everyone", allow_permissions: 0, deny_permissions: SCREEN_SHARE },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.deny).toBe(SCREEN_SHARE | PermissionBits.SEND_MESSAGES);
    expect(body.allow).toBe(0);
  });

  it("a listed but hidden permission is neither cleared nor set on save", async () => {
    // Hidden: stored in both masks (shown it would be read as "allow" and
    // saved as allow only); a save must leave both copies as stored.
    p.hidden.add("CREATE_INVITE");
    p.overrides = [
      {
        role_id: "everyone",
        allow_permissions: PermissionBits.CREATE_INVITE,
        deny_permissions: PermissionBits.CREATE_INVITE,
      },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.allow).toBe(PermissionBits.CREATE_INVITE);
    expect(body.deny).toBe(PermissionBits.CREATE_INVITE + PermissionBits.SEND_MESSAGES);
  });

  it("keeps every stored bit the editor does not list, also above bit 31", async () => {
    const unlistedAllow = PermissionBits.MANAGE_EVENTS + HIGH * 2;
    const unlistedDeny = PermissionBits.MANAGE_POSTS + PermissionBits.SEND_ANNOUNCEMENTS + HIGH;
    p.roles = [MODS];
    p.overrides = [
      {
        role_id: "mods",
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

describe("ChannelPermissions for @everyone", () => {
  const rowOf = (key: string) =>
    screen.getByTestId(`channel-permissions-row-${key.toLowerCase().replace(/_/g, "-")}`);
  const allowOf = (key: string) =>
    within(rowOf(key)).getByTestId("channel-permissions-allow") as HTMLInputElement;

  it("does not offer allow for a permission @everyone may not get; deny still saves", async () => {
    p.overrides = [{ role_id: "everyone", allow_permissions: 0, deny_permissions: 0 }];
    render(() => <ChannelPermissions channelId="c1" guildId="g1" />);
    fireEvent.click(screen.getByTestId("channel-permissions-override-edit"));

    expect(allowOf("MANAGE_MESSAGES").disabled).toBe(true);
    const manage = rowOf("MANAGE_MESSAGES");
    expect((within(manage).getByTestId("channel-permissions-deny") as HTMLInputElement).disabled).toBe(false);
    expect((within(manage).getByTestId("channel-permissions-inherit") as HTMLInputElement).disabled).toBe(false);
    // A permission @everyone may hold keeps its allow.
    expect(allowOf("SEND_MESSAGES").disabled).toBe(false);

    fireEvent.click(within(manage).getByTestId("channel-permissions-deny"));
    fireEvent.click(screen.getByTestId("channel-permissions-save"));
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.deny).toBe(PermissionBits.MANAGE_MESSAGES);
    expect(body.allow).toBe(0);
  });

  it("a save never sends an allow @everyone may not get, listed or not", async () => {
    // Stored earlier: allow Manage Messages (listed, refused for @everyone),
    // allow Manage Events (not listed, refused for @everyone), allow Attach
    // Files (fine) and a bit the table does not know.
    p.overrides = [
      {
        role_id: "everyone",
        allow_permissions:
          PermissionBits.MANAGE_MESSAGES +
          PermissionBits.MANAGE_EVENTS +
          PermissionBits.ATTACH_FILES +
          HIGH,
        deny_permissions: 0,
      },
    ];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.allow).toBe(PermissionBits.ATTACH_FILES + HIGH);
    expect(body.deny).toBe(PermissionBits.SEND_MESSAGES);
  });

  it("another role's override sends the same stored allows back", async () => {
    const stored = PermissionBits.MANAGE_MESSAGES + PermissionBits.MANAGE_EVENTS + HIGH;
    p.roles = [MODS];
    p.overrides = [{ role_id: "mods", allow_permissions: stored, deny_permissions: 0 }];
    openEditorAndDenySending();
    await waitFor(() => expect(p.setChannelOverride).toHaveBeenCalled());
    const [, , body] = p.setChannelOverride.mock.calls[0];
    expect(body.allow).toBe(stored);
    expect(body.deny).toBe(PermissionBits.SEND_MESSAGES);
  });

  it("a normal role keeps allow for the same permission", () => {
    p.roles = [{ id: "mods", name: "Mods", is_default: false, permissions: 0, color: null }];
    p.overrides = [{ role_id: "mods", allow_permissions: 0, deny_permissions: 0 }];
    render(() => <ChannelPermissions channelId="c1" guildId="g1" />);
    fireEvent.click(screen.getByTestId("channel-permissions-override-edit"));
    expect(allowOf("MANAGE_MESSAGES").disabled).toBe(false);
  });
});
