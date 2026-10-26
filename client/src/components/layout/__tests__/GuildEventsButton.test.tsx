import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));

import * as tauri from "@/lib/tauri";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import GuildEventsButton from "../GuildEventsButton";

const withEvents = (on: boolean) =>
  vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
    features: {
      forum_channels: false,
      announcement_channels: false,
      scheduled_events: on,
      membership_screening: false,
    },
  } as never);

describe("GuildEventsButton", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    resetServerFeatures();
  });

  it("events entry hidden when scheduled_events is off", async () => {
    withEvents(false);
    await loadServerFeatures();
    render(() => <GuildEventsButton onClick={() => {}} />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows the events entry when scheduled_events is on", async () => {
    withEvents(true);
    await loadServerFeatures();
    render(() => <GuildEventsButton onClick={() => {}} />);
    expect(screen.getByRole("button")).toBeTruthy();
  });
});
