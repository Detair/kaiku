import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock, tauriMode } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  tauriMode: { native: false },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: () => tauriMode.native,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

const fetchMock = vi.fn();

function respond(status: number, body: unknown = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: "",
    json: vi.fn().mockResolvedValue(body),
    text: vi.fn().mockResolvedValue(JSON.stringify(body)),
  };
}

type Api = Record<string, (...args: string[]) => Promise<unknown>>;

interface Case {
  fn: string;
  module: string;
  args: string[];
  method: string;
  path: string;
  oldCommand: string;
}

const CASES: Case[] = [
  { fn: "getGuilds", module: "guilds", args: [], method: "GET", path: "/api/guilds", oldCommand: "get_guilds" },
  { fn: "getDMList", module: "dms", args: [], method: "GET", path: "/api/dm", oldCommand: "get_dm_list" },
  { fn: "deleteMessage", module: "messages", args: ["m"], method: "DELETE", path: "/api/messages/m", oldCommand: "delete_message" },
  { fn: "addReaction", module: "messages", args: ["c", "m", "👍"], method: "PUT", path: "/api/channels/c/messages/m/reactions", oldCommand: "add_reaction" },
  { fn: "markChannelAsRead", module: "channels", args: ["c", "m"], method: "POST", path: "/api/channels/c/read", oldCommand: "mark_channel_as_read" },
  { fn: "adminListUsers", module: "admin", args: [], method: "GET", path: "/api/admin/users", oldCommand: "admin_list_users" },
  { fn: "adminDeElevate", module: "admin", args: [], method: "DELETE", path: "/api/admin/elevate", oldCommand: "admin_de_elevate" },
  { fn: "getMessages", module: "messages", args: ["c"], method: "GET", path: "/api/messages/channel/c", oldCommand: "get_messages" },
  { fn: "mfaSetup", module: "auth", args: [], method: "POST", path: "/auth/mfa/setup", oldCommand: "mfa_setup" },
  { fn: "qrLoginCreate", module: "auth", args: [], method: "POST", path: "/auth/qr/create", oldCommand: "qr_login_create" },
];

const MODULES: Record<string, () => Promise<unknown>> = {
  guilds: () => import("../tauri/guilds"),
  dms: () => import("../tauri/dms"),
  messages: () => import("../tauri/messages"),
  channels: () => import("../tauri/channels"),
  admin: () => import("../tauri/admin"),
  auth: () => import("../tauri/auth"),
};

describe("native REST transport", () => {
  beforeEach(() => {
    invokeMock.mockReset().mockRejectedValue(new Error("unexpected invoke"));
    listenMock.mockReset().mockResolvedValue(() => {});
    fetchMock.mockReset().mockResolvedValue(respond(200, {}));
    vi.stubGlobal("fetch", fetchMock);
    tauriMode.native = true;
    vi.resetModules();
  });

  afterEach(() => {
    tauriMode.native = false;
    vi.unstubAllGlobals();
  });

  it.each(CASES)(
    "$fn issues one $method $path with the bearer",
    async ({ fn, module, args, method, path, oldCommand }) => {
      const common = await import("../tauri/common");
      common.setNativeAuthInfo({
        server_url: "https://srv",
        access_token: "tok",
        expires_at_ms: Date.now() + 900_000,
      });
      const api = (await MODULES[module]()) as Api;

      await api[fn](...args);

      expect(fetchMock).toHaveBeenCalledTimes(1);
      const [url, init] = fetchMock.mock.calls[0] as [
        string,
        { method: string; headers: Record<string, string> },
      ];
      expect(new URL(url).origin).toBe("https://srv");
      expect(new URL(url).pathname).toBe(path);
      expect(init.method).toBe(method);
      expect(init.headers["Authorization"]).toBe("Bearer tok");
      const commands = invokeMock.mock.calls.map((c) => c[0]);
      expect(commands).not.toContain(oldCommand);
    },
  );

  it("sendMessageWithStatus posts over HTTP and keeps the response status", async () => {
    const common = await import("../tauri/common");
    common.setNativeAuthInfo({
      server_url: "https://srv",
      access_token: "tok",
      expires_at_ms: Date.now() + 900_000,
    });
    fetchMock.mockResolvedValue(respond(202, { id: "m1" }));
    const messages = await import("../tauri/messages");

    const result = await messages.sendMessageWithStatus("c", "/roll");

    expect(result).toEqual({ message: { id: "m1" }, status: 202 });
    const [url, init] = fetchMock.mock.calls[0] as [
      string,
      { method: string; headers: Record<string, string>; body: string },
    ];
    expect(url).toBe("https://srv/api/messages/channel/c");
    expect(init.method).toBe("POST");
    expect(init.headers["Authorization"]).toBe("Bearer tok");
    expect(JSON.parse(init.body)).toMatchObject({ content: "/roll" });
    expect(invokeMock).not.toHaveBeenCalled();
  });
  it("403 PASSWORD_CHANGE_REQUIRED dispatches kaiku:password-change-required and throws a coded HttpError", async () => {
    const common = await import("../tauri/common");
    common.setNativeAuthInfo({
      server_url: "https://srv",
      access_token: "tok",
      expires_at_ms: Date.now() + 900_000,
    });
    fetchMock.mockResolvedValue(
      respond(403, { error: "PASSWORD_CHANGE_REQUIRED", message: "Password change required" }),
    );
    const onRequired = vi.fn();
    window.addEventListener("kaiku:password-change-required", onRequired);
    try {
      const error = await common.httpRequest("GET", "/api/guilds").catch((e: unknown) => e);

      expect(error).toBeInstanceOf(common.HttpError);
      expect((error as InstanceType<typeof common.HttpError>).status).toBe(403);
      expect((error as Error).message).toBe("PASSWORD_CHANGE_REQUIRED");
      expect(onRequired).toHaveBeenCalledTimes(1);
      // No refresh attempt: the token is fine, the account is not.
      expect(fetchMock).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("kaiku:password-change-required", onRequired);
    }
  });

  it("other 403s do not report a pending password change", async () => {
    const common = await import("../tauri/common");
    common.setNativeAuthInfo({
      server_url: "https://srv",
      access_token: "tok",
      expires_at_ms: Date.now() + 900_000,
    });
    fetchMock.mockResolvedValue(respond(403, { error: "FORBIDDEN", message: "Forbidden" }));
    const onRequired = vi.fn();
    window.addEventListener("kaiku:password-change-required", onRequired);
    try {
      await expect(common.httpRequest("GET", "/api/guilds")).rejects.toThrow("Forbidden");
      expect(onRequired).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("kaiku:password-change-required", onRequired);
    }
  });
});
