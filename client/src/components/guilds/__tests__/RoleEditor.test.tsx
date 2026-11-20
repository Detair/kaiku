import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import type { GuildRole } from "@/lib/types";

vi.mock("@/stores/permissions", () => ({
  permissionsState: { memberRoles: {} },
  createRole: vi.fn(),
  updateRole: vi.fn(),
  getMemberRoles: () => [],
  assignMemberRole: vi.fn(),
  removeMemberRole: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "u1" } } }));
vi.mock("@/stores/guilds", () => ({ isGuildOwner: () => true, getGuildMembers: () => [] }));
vi.mock("@/stores/serverFeatures", () => ({ isPermissionVisible: () => true }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import RoleEditor from "../RoleEditor";

function role(id: string, name: string): GuildRole {
  return {
    id,
    guild_id: "g1",
    name,
    color: null,
    permissions: 0,
    position: 1,
    is_default: false,
    created_at: "2026-01-01T00:00:00Z",
  } as GuildRole;
}

afterEach(() => cleanup());

describe("RoleEditor", () => {
  it("keeps unsaved edits when the edited role is updated in the store", () => {
    const [state, setState] = createStore({ role: role("r1", "Mods") });
    render(() => (
      <RoleEditor guildId="g1" role={state.role} onBack={() => {}} onSave={() => {}} />
    ));
    const input = screen.getByTestId("role-editor-name-input") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "Moderators" } });

    setState("role", "permissions", 8);
    setState("role", "position", 2);

    expect(input.value).toBe("Moderators");
  });

  it("keeps unsaved edits when the role is replaced by a fresh copy of itself", () => {
    const [current, setCurrent] = createSignal<GuildRole>(role("r1", "Mods"));
    render(() => (
      <RoleEditor guildId="g1" role={current()} onBack={() => {}} onSave={() => {}} />
    ));
    const input = screen.getByTestId("role-editor-name-input") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "Moderators" } });

    setCurrent({ ...role("r1", "Mods"), permissions: 8 });

    expect(input.value).toBe("Moderators");
  });

  it("loads the form again when a different role is opened", () => {
    const [state, setState] = createStore<{ role: GuildRole }>({ role: role("r1", "Mods") });
    render(() => (
      <RoleEditor guildId="g1" role={state.role} onBack={() => {}} onSave={() => {}} />
    ));
    const input = screen.getByTestId("role-editor-name-input") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "Moderators" } });

    setState("role", role("r2", "Helpers"));

    expect(input.value).toBe("Helpers");
  });

  it("@everyone shows screen sharing (on by default) and hides no-longer-emoji bit 22", () => {
    const everyone = { ...role("e", "@everyone"), is_default: true, permissions: (1 << 22) | (1 << 5) };
    render(() => <RoleEditor guildId="g1" role={everyone} onBack={() => {}} onSave={() => {}} />);
    const box = screen.getByTestId("role-editor-perm-screen-share") as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(screen.queryByText(/Manage Emojis/)).toBeNull();
    // Moderation-only permissions stay off the @everyone editor.
    expect(screen.queryByTestId("role-editor-perm-manage-posts")).toBeNull();
  });

  it("a normal role offers forum, announcement and event permissions", () => {
    render(() => <RoleEditor guildId="g1" role={role("r1", "Mods")} onBack={() => {}} onSave={() => {}} />);
    expect(screen.getByTestId("role-editor-perm-manage-posts")).toBeTruthy();
    expect(screen.getByTestId("role-editor-perm-send-announcements")).toBeTruthy();
    expect(screen.getByTestId("role-editor-perm-manage-events")).toBeTruthy();
  });
});
