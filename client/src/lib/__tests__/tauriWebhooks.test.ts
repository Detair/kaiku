import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createWebhook,
  deleteWebhook,
  listGuildWebhooks,
  updateWebhook,
} from "../tauri/webhooks";
import { PermissionBits } from "../permissionConstants";

const WEBHOOK = {
  id: "wh-1",
  type: 1,
  guild_id: "g-1",
  channel_id: "c-1",
  name: "Game Server",
  avatar: null,
  token: "t".repeat(68),
  application_id: null,
  url: "https://kaiku.example.com/api/webhooks/wh-1/" + "t".repeat(68),
};

function mockFetch(body: unknown, status = 200) {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: status < 400,
    status,
    statusText: "OK",
    headers: new Headers({ "content-type": "application/json" }),
    json: vi.fn().mockResolvedValue(body),
    text: vi.fn().mockResolvedValue(JSON.stringify(body)),
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("incoming webhooks API", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("lists guild webhooks via GET /api/guilds/{id}/webhooks", async () => {
    const fetchMock = mockFetch([WEBHOOK]);
    const result = await listGuildWebhooks("g-1");
    expect(result).toHaveLength(1);
    expect(result[0].url).toContain("/api/webhooks/wh-1/");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/api/guilds/g-1/webhooks");
    expect(init.method).toBe("GET");
  });

  it("creates a webhook via POST /api/channels/{id}/webhooks", async () => {
    const fetchMock = mockFetch(WEBHOOK);
    const result = await createWebhook("c-1", { name: "Game Server" });
    expect(result.token.length).toBeGreaterThanOrEqual(60);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/api/channels/c-1/webhooks");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({ name: "Game Server" });
  });

  it("updates a webhook via PATCH /api/webhooks/{id}", async () => {
    const fetchMock = mockFetch({ ...WEBHOOK, name: "Renamed" });
    const result = await updateWebhook("wh-1", { name: "Renamed" });
    expect(result.name).toBe("Renamed");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/api/webhooks/wh-1");
    expect(init.method).toBe("PATCH");
  });

  it("deletes a webhook via DELETE /api/webhooks/{id}", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 204,
      statusText: "No Content",
      headers: new Headers(),
      json: vi.fn().mockRejectedValue(new Error("no body")),
      text: vi.fn().mockResolvedValue(""),
    });
    vi.stubGlobal("fetch", fetchMock);
    await deleteWebhook("wh-1");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/api/webhooks/wh-1");
    expect(init.method).toBe("DELETE");
  });
});

describe("MANAGE_WEBHOOKS permission bit", () => {
  it("matches the server-side bit (1 << 29)", () => {
    expect(PermissionBits.MANAGE_WEBHOOKS).toBe(1 << 29);
  });
});
