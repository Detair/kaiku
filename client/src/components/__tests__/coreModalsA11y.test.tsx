import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";

// --- Heavy dependencies, replaced with inert stand-ins -------------------
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke: vi.fn(),
}));
vi.mock("@/lib/tauri", () => ({
  initE2EE: vi.fn(),
  qrLoginCreate: vi.fn(() => new Promise(() => {})),
  getServerUrl: () => "https://chat.example",
  HttpError: class HttpError extends Error {},
  discoverGuilds: vi.fn(async () => ({ guilds: [] })),
  joinDiscoverable: vi.fn(),
}));
vi.mock("@/lib/featureFlags", () => ({ isE2EEEnabled: () => false }));
vi.mock("qrcode", () => ({ default: { toDataURL: vi.fn() } }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("../settings/AccountSettings", () => ({ default: () => null }));
vi.mock("../settings/AppearanceSettings", () => ({ default: () => null }));
vi.mock("../settings/NotificationSettings", () => ({ default: () => null }));
vi.mock("../settings/SecuritySettings", () => ({ default: () => null }));
vi.mock("../settings/PrivacySettings", () => ({ default: () => null }));
vi.mock("../settings/FocusSettings", () => ({ default: () => null }));
vi.mock("../settings/RecoveryKeyModal", () => ({ default: () => null }));
vi.mock("../settings/AudioSettings", () => ({ default: () => null }));
vi.mock("../settings/VoiceSettings", () => ({ default: () => null }));
vi.mock("../channels/ChannelPermissions", () => ({ default: () => null }));
vi.mock("../voice/MicTestPanel", () => ({ default: () => null }));

vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "c1", name: "general", channel_type: "text" }] },
  createChannel: vi.fn(),
}));
vi.mock("@/stores/categories", () => ({ getCategory: () => undefined }));
vi.mock("@/stores/permissions", () => ({ memberHasPermission: () => false }));
vi.mock("@/stores/guilds", () => ({
  guildsState: { guilds: [] },
  isGuildOwner: () => false,
  joinViaInviteCode: vi.fn(),
}));
vi.mock("@/stores/sound", () => ({
  getChannelNotificationLevel: () => "all",
  setChannelNotificationLevel: vi.fn(),
}));
vi.mock("@/stores/admin", () => ({
  adminState: { isElevated: true },
  createUser: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({
  authState: { user: { id: "u1", display_name: "Ann" }, isInitialized: true, setupRequired: false },
  currentUser: () => ({ id: "u1", display_name: "Ann" }),
  updateUser: vi.fn(),
}));
vi.mock("@/stores/preferences", () => ({
  preferences: () => ({ onboarding_completed: false }),
  updatePreference: vi.fn(),
  isInitialized: () => true,
}));
vi.mock("@/stores/theme", () => ({
  setTheme: vi.fn(),
  pickerThemes: () => [],
  theme: () => "dark",
}));
vi.mock("@/lib/themeImage", () => ({ getThemeImage: () => "" }));
vi.mock("@/assets/images/floki_onboard_done.webp", () => ({ default: "" }));

import SettingsModal from "../settings/SettingsModal";
import QrLoginModal from "../settings/QrLoginModal";
import OnboardingWizard from "../OnboardingWizard";
import CreateChannelModal from "../channels/CreateChannelModal";
import ChannelSettingsModal from "../channels/ChannelSettingsModal";
import CreateAccountModal from "../admin/CreateAccountModal";

describe("core modals expose role dialog with a label", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("SettingsModal", () => {
    render(() => <SettingsModal onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Settings" })).toBeTruthy();
  });

  it("QrLoginModal", () => {
    render(() => <QrLoginModal serverUrl="https://chat.example" onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Link Mobile Device" })).toBeTruthy();
  });

  it("OnboardingWizard", () => {
    render(() => <OnboardingWizard />);
    expect(screen.getByRole("dialog", { name: "Onboarding wizard" })).toBeTruthy();
  });

  it("CreateChannelModal", () => {
    render(() => <CreateChannelModal guildId="g1" onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Create Channel" })).toBeTruthy();
  });

  it("ChannelSettingsModal", () => {
    render(() => <ChannelSettingsModal channelId="c1" guildId="g1" onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "general" })).toBeTruthy();
  });

  it("CreateAccountModal", () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Create account" })).toBeTruthy();
  });
});

describe("Escape handling", () => {
  it("Escape inside settings closes it and does not reach document listeners (channel stays unread)", () => {
    const onClose = vi.fn();
    const markRead = vi.fn(); // stands in for MessageList's document-level handler
    document.addEventListener("keydown", markRead);
    try {
      render(() => <SettingsModal onClose={onClose} />);
      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(markRead).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", markRead);
    }
  });

  it("QrLoginModal registers only one Escape handler (onClose once)", () => {
    const onClose = vi.fn();
    render(() => <QrLoginModal serverUrl="https://chat.example" onClose={onClose} />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("CreateAccountModal success view", () => {
  it("Escape closes the form but not the one-time success view", async () => {
    const admin = await import("@/stores/admin");
    (admin.createUser as ReturnType<typeof vi.fn>).mockResolvedValue({
      username: "bob",
      display_name: "Bob",
    });
    const onClose = vi.fn();
    render(() => <CreateAccountModal onClose={onClose} />);
    const dialog = screen.getByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    const input = dialog.querySelector<HTMLInputElement>("input[type=text]")!;
    fireEvent.input(input, { target: { value: "bob" } });
    fireEvent.submit(dialog.querySelector("form")!);
    await screen.findByText("Account created");
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
