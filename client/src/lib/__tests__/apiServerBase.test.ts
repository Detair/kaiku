import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const fetchMock = vi.fn();

function ok(body: unknown, status = 200) {
  return {
    ok: true,
    status,
    statusText: "OK",
    text: vi.fn().mockResolvedValue(body === null ? "" : JSON.stringify(body)),
  };
}

function fail(status: number, body: unknown) {
  return {
    ok: false,
    status,
    statusText: "",
    text: vi.fn().mockResolvedValue(JSON.stringify(body)),
  };
}

function expectServerCall(index = 0) {
  const [url, init] = fetchMock.mock.calls[index];
  expect(String(url)).not.toContain("localhost:3000");
  expect(String(url).startsWith("https://srv/api/")).toBe(true);
  expect(init.headers.Authorization).toBe("Bearer tok");
  return { url: String(url), init };
}

describe("bots/webhooks/filters API use the configured server (C16)", () => {
  beforeEach(async () => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    vi.resetModules();
    const common = await import("../tauri/common");
    common.setNativeAuthInfo({
      server_url: "https://srv",
      access_token: "tok",
      expires_at_ms: Date.now() + 900_000,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("bots: listGuildCommands goes to the server", async () => {
    fetchMock.mockResolvedValue(ok([]));
    const bots = await import("../api/bots");
    await expect(bots.listGuildCommands("g-1")).resolves.toEqual([]);
    expect(expectServerCall().url).toBe("https://srv/api/guilds/g-1/commands");
  });

  it("bots: query string is kept for scoped commands", async () => {
    fetchMock.mockResolvedValue(ok([]));
    const bots = await import("../api/bots");
    await bots.listSlashCommands("app-1", "g 1");
    expect(expectServerCall().url).toBe(
      "https://srv/api/applications/app-1/commands?guild_id=g%201",
    );
  });

  it("bots: 204 delete resolves and errors carry the code", async () => {
    fetchMock.mockResolvedValueOnce(ok(null, 204));
    const bots = await import("../api/bots");
    await expect(bots.deleteBotApplication("app-1")).resolves.toBeUndefined();
    expect(expectServerCall().init.method).toBe("DELETE");

    fetchMock.mockResolvedValueOnce(fail(403, { error: "FORBIDDEN", message: "No" }));
    const err = await bots.createBotApplication({ name: "x" }).catch((e: unknown) => e);
    const common = await import("../tauri/common");
    expect(err).toBeInstanceOf(common.HttpError);
    expect((err as InstanceType<typeof common.HttpError>).code).toBe("FORBIDDEN");
  });

  it("webhooks: createWebhook posts JSON to the server and surfaces the code", async () => {
    fetchMock.mockResolvedValueOnce(ok({ id: "w1", signing_secret: "s" }));
    const webhooks = await import("../api/webhooks");
    await webhooks.createWebhook("app-1", {
      url: "https://hook.example",
      subscribed_events: ["message.created"],
    });
    const { url, init } = expectServerCall();
    expect(url).toBe("https://srv/api/applications/app-1/webhooks");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body).url).toBe("https://hook.example");

    fetchMock.mockResolvedValueOnce(fail(400, { error: "VALIDATION_ERROR", message: "bad" }));
    const err = await webhooks.listWebhooks("app-1").catch((e: unknown) => e);
    expect((err as { code?: string }).code).toBe("VALIDATION_ERROR");
  });

  it("filters: list, log query string and error code", async () => {
    fetchMock.mockResolvedValueOnce(ok([]));
    const filters = await import("../api/filters");
    await filters.listFilterConfigs("g-1");
    expect(expectServerCall().url).toBe("https://srv/api/guilds/g-1/filters");

    fetchMock.mockResolvedValueOnce(ok({ items: [], total: 0, limit: 10, offset: 5 }));
    await filters.listModerationLog("g-1", 10, 5);
    expect(expectServerCall(1).url).toBe(
      "https://srv/api/guilds/g-1/filters/log?limit=10&offset=5",
    );

    fetchMock.mockResolvedValueOnce(fail(403, { error: "FORBIDDEN", message: "No" }));
    const err = await filters.updateFilterConfigs("g-1", []).catch((e: unknown) => e);
    expect((err as { code?: string }).code).toBe("FORBIDDEN");
  });
});
