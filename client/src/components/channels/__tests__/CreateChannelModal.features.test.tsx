import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));
vi.mock("@/stores/channels", () => ({ createChannel: vi.fn() }));
vi.mock("@/stores/categories", () => ({ getCategory: vi.fn() }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import * as tauri from "@/lib/tauri";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import CreateChannelModal from "../CreateChannelModal";

const features = (f: Partial<Record<string, boolean>>) =>
  vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
    features: {
      forum_channels: false,
      announcement_channels: false,
      scheduled_events: false,
      membership_screening: false,
      ...f,
    },
  } as never);

describe("CreateChannelModal optional channel types", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    resetServerFeatures();
  });

  it("create channel modal offers only text and voice when disabled", async () => {
    features({});
    await loadServerFeatures();
    render(() => <CreateChannelModal guildId="g1" onClose={() => {}} />);
    expect(screen.getByText("Voice")).toBeTruthy();
    expect(screen.queryByText("Forum")).toBeNull();
    expect(screen.queryByText("Announcement")).toBeNull();
  });

  it("offers forum and announcement when enabled", async () => {
    features({ forum_channels: true, announcement_channels: true });
    await loadServerFeatures();
    render(() => <CreateChannelModal guildId="g1" onClose={() => {}} />);
    expect(screen.getByText("Forum")).toBeTruthy();
    expect(screen.getByText("Announcement")).toBeTruthy();
  });
});
