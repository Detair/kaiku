import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
  listGuildWebhooks: vi.fn().mockResolvedValue([]),
  createWebhook: vi.fn(),
  updateWebhook: vi.fn(),
  deleteWebhook: vi.fn(),
}));
vi.mock("@/stores/channels", async () => {
  const actual = await vi.importActual<typeof import("@/stores/channels")>("@/stores/channels");
  const ch = (id: string, name: string, channel_type: string, position: number) => ({
    id, name, channel_type, position,
  });
  return {
    ...actual,
    channelsState: {
      channels: [ch("t", "general", "text", 0), ch("f", "forumchan", "forum", 1), ch("a", "annchan", "announcement", 2)],
    },
  };
});

import * as tauri from "@/lib/tauri";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import WebhooksTab from "../WebhooksTab";

describe("WebhooksTab targets", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    resetServerFeatures();
  });

  it("does not offer forum or announcement channels when they are off", async () => {
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
      features: { forum_channels: false, announcement_channels: false, scheduled_events: false, membership_screening: false },
    } as never);
    await loadServerFeatures();
    render(() => <WebhooksTab guildId="g1" />);
    expect(screen.queryByText("#general")).not.toBeNull();
    expect(screen.queryByText("#forumchan")).toBeNull();
    expect(screen.queryByText("#annchan")).toBeNull();
  });
});
