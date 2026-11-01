import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));
vi.mock("@/stores/channels", () => ({ createChannel: vi.fn() }));
vi.mock("@/stores/categories", () => ({ getCategory: vi.fn() }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import CreateChannelModal from "../CreateChannelModal";

describe("CreateChannelModal states", () => {
  beforeEach(() => cleanup());

  it("placeholder reads as an example, not a typed value", () => {
    render(() => <CreateChannelModal guildId="g1" initialType="voice" onClose={() => {}} />);
    const input = screen.getByTestId("create-channel-name") as HTMLInputElement;
    expect(input.value).toBe("");
    expect(input.placeholder).toBe("e.g. voice-lounge");
  });

  it("says why Create Channel is disabled and clears the reason once a name is typed", () => {
    render(() => <CreateChannelModal guildId="g1" onClose={() => {}} />);
    const submit = screen.getByTestId("create-channel-submit") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    const hint = screen.getByText("Enter a channel name to continue.");
    expect(submit.getAttribute("aria-describedby")).toBe(hint.id);

    fireEvent.input(screen.getByTestId("create-channel-name"), { target: { value: "lobby" } });
    expect(submit.disabled).toBe(false);
    expect(screen.queryByText("Enter a channel name to continue.")).toBeNull();
  });

  it("marks the selected channel type for assistive tech", () => {
    render(() => <CreateChannelModal guildId="g1" onClose={() => {}} />);
    const text = screen.getByRole("button", { name: /Text/ });
    const voice = screen.getByRole("button", { name: /Voice/ });
    expect(text.getAttribute("aria-pressed")).toBe("true");
    expect(voice.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(voice);
    expect(voice.getAttribute("aria-pressed")).toBe("true");
    expect(text.getAttribute("aria-pressed")).toBe("false");
  });
});
