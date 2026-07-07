import { describe, it, expect, beforeEach } from "vitest";
import { applyMemberRoles, getMember } from "@/stores/members";
import { guildsState, setGuildsState } from "@/stores/guilds";

const GUILD = "11111111-1111-1111-1111-111111111111";
const USER = "22222222-2222-2222-2222-222222222222";

describe("applyMemberRoles", () => {
  beforeEach(() => {
    setGuildsState("members", GUILD, [
      {
        user_id: USER,
        username: "alice",
        display_name: "Alice",
        avatar_url: null,
        nickname: null,
        joined_at: "2026-01-01T00:00:00Z",
        status: "online",
        last_seen_at: null,
      },
    ]);
  });

  it("updates the cached member's role_ids", () => {
    applyMemberRoles(GUILD, USER, ["role-a", "role-b"]);
    expect(getMember(GUILD, USER)?.role_ids).toEqual(["role-a", "role-b"]);
  });

  it("ignores unknown guilds without throwing", () => {
    expect(() => applyMemberRoles("no-such-guild", USER, ["x"])).not.toThrow();
  });

  it("ignores unknown members without throwing", () => {
    applyMemberRoles(GUILD, "no-such-user", ["x"]);
    expect(guildsState.members[GUILD][0].role_ids).toBeUndefined();
  });
});
