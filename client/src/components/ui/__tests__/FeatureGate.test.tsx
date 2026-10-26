import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));

import * as tauri from "@/lib/tauri";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import FeatureGate from "../FeatureGate";

const flags = (on: boolean) =>
  vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
    features: {
      forum_channels: on,
      announcement_channels: on,
      scheduled_events: on,
      membership_screening: on,
    },
  } as never);

describe("FeatureGate", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    resetServerFeatures();
  });

  it("screening gate stays mounted while flags are unknown, goes when off", async () => {
    render(() => (
      <FeatureGate feature="membership_screening" whileUnknown>
        <p>gate</p>
      </FeatureGate>
    ));
    expect(screen.queryByText("gate")).not.toBeNull();
    flags(false);
    await loadServerFeatures();
    expect(screen.queryByText("gate")).toBeNull();
  });

  it("events modal guard hides while unknown and when off, shows when on", async () => {
    render(() => (
      <FeatureGate feature="scheduled_events">
        <p>events</p>
      </FeatureGate>
    ));
    expect(screen.queryByText("events")).toBeNull();
    flags(true);
    await loadServerFeatures();
    expect(screen.queryByText("events")).not.toBeNull();
  });
});
