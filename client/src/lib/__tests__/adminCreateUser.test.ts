import { beforeEach, describe, expect, it, vi } from "vitest";
import { adminCreateUser } from "../tauri";
import { HttpError } from "../tauri/common";

const summary = {
  id: "u1",
  username: "anna",
  display_name: "Anna",
  email: null,
  avatar_url: null,
  created_at: "2026-10-24T00:00:00Z",
  is_banned: false,
};

describe("adminCreateUser", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("posts the contract body", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, status: 201, text: vi.fn().mockResolvedValue(JSON.stringify(summary)) }),
    );
    const req = { username: "anna", display_name: "Anna", temporary_password: "abcdEFGH2345" };
    const result = await adminCreateUser(req);
    expect(result).toEqual(summary);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringMatching(/\/api\/admin\/users$/),
      expect.objectContaining({ method: "POST", body: JSON.stringify(req) }),
    );
  });

  it("surfaces 409 as HttpError", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        statusText: "Conflict",
        text: vi.fn().mockResolvedValue("Username already taken"),
      }),
    );
    const err = await adminCreateUser({
      username: "anna",
      display_name: null,
      temporary_password: "abcdEFGH2345",
    }).catch((e) => e);
    expect(err).toBeInstanceOf(HttpError);
    expect(err.status).toBe(409);
  });
});
