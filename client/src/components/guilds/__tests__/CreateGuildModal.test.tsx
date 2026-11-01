import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
  uploadGuildBanner: vi.fn(),
}));
vi.mock("@/stores/guilds", () => ({
  createGuild: vi.fn(async () => ({ id: "g1" })),
}));

import * as tauri from "@/lib/tauri";
import { createGuild } from "@/stores/guilds";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import CreateGuildModal from "../CreateGuildModal";

const settings = (guildDiscovery?: boolean) =>
  vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
    ...(guildDiscovery === undefined ? {} : { guild_discovery: guildDiscovery }),
  } as never);

const typeName = (value: string) =>
  fireEvent.input(screen.getByTestId("create-guild-name"), { target: { value } });

describe("CreateGuildModal", () => {
  beforeEach(() => {
    cleanup();
    resetServerFeatures();
    vi.mocked(createGuild).mockClear();
  });

  it("explains why Next is disabled while the name is empty", async () => {
    settings(true);
    await loadServerFeatures();
    render(() => <CreateGuildModal onClose={() => {}} />);
    const next = screen.getByTestId("create-guild-next") as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    expect(screen.getByText("Enter a server name to continue.")).toBeTruthy();
    typeName("Game Night");
    expect(next.disabled).toBe(false);
    expect(screen.queryByText("Enter a server name to continue.")).toBeNull();
  });

  it("shows example placeholders, not values", async () => {
    settings(true);
    await loadServerFeatures();
    render(() => <CreateGuildModal onClose={() => {}} />);
    const input = screen.getByTestId("create-guild-name") as HTMLInputElement;
    expect(input.placeholder.startsWith("e.g. ")).toBe(true);
  });

  it("offers the discovery step with a real switch when discovery is on", async () => {
    settings(true);
    await loadServerFeatures();
    render(() => <CreateGuildModal onClose={() => {}} />);
    typeName("Game Night");
    fireEvent.click(screen.getByTestId("create-guild-next"));
    const sw = screen.getByRole("switch", { name: "Make server discoverable" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(sw);
    expect(sw.getAttribute("aria-checked")).toBe("true");
    // The themed file picker replaces the native control.
    expect(screen.getByRole("button", { name: "Choose image" })).toBeTruthy();
    expect(screen.getByText("No file chosen")).toBeTruthy();
  });

  it("skips the discovery step when the instance has discovery off", async () => {
    settings(false);
    await loadServerFeatures();
    const onClose = vi.fn();
    render(() => <CreateGuildModal onClose={onClose} />);
    expect(screen.getByTestId("create-guild-discovery-off")).toBeTruthy();
    expect(screen.queryByTestId("create-guild-next")).toBeNull();
    typeName("Game Night");
    fireEvent.click(screen.getByTestId("create-guild-submit"));
    await vi.waitFor(() => expect(createGuild).toHaveBeenCalledWith("Game Night", undefined, undefined));
    expect(screen.queryByRole("switch")).toBeNull();
  });
});
