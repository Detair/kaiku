import { describe, it, expect, vi, beforeEach } from "vitest";
import { createSignal, Show } from "solid-js";
import { render, screen, fireEvent } from "@solidjs/testing-library";

// --- Heavy dependencies, replaced with inert stand-ins -------------------
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke: vi.fn(),
}));
vi.mock("@/lib/tauri", () => ({
  initE2EE: vi.fn(),
  getServerUrl: () => "https://chat.example",
  HttpError: class HttpError extends Error {},
  discoverGuilds: vi.fn(async () => ({ guilds: [] })),
  joinDiscoverable: vi.fn(),
  createReport: vi.fn(() => new Promise(() => {})),
}));
vi.mock("@/stores/friends", () => ({ blockUser: vi.fn(() => new Promise(() => {})) }));
vi.mock("@floating-ui/dom", () => ({
  computePosition: vi.fn(async () => ({ x: 0, y: 0 })),
  autoUpdate: vi.fn((_a: unknown, _b: unknown, update: () => void) => {
    update();
    return () => {};
  }),
  flip: vi.fn(() => ({})),
  shift: vi.fn(() => ({})),
  offset: vi.fn(() => ({})),
  size: vi.fn(() => ({})),
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
vi.mock("@/stores/permissions", () => ({
  memberHasPermission: () => false,
  getGuildRoles: () => [],
  getMemberRoleIds: () => [],
  getChannelOverrides: () => [],
  getLoadedChannelOverrides: () => [],
  loadChannelOverrides: vi.fn(),
}));
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
import OnboardingWizard from "../OnboardingWizard";
import CreateChannelModal from "../channels/CreateChannelModal";
import ChannelSettingsModal from "../channels/ChannelSettingsModal";
import CreateAccountModal from "../admin/CreateAccountModal";
import ReportModal from "../modals/ReportModal";
import BlockConfirmModal from "../modals/BlockConfirmModal";
import ImageLightbox from "../ui/ImageLightbox";
import PositionedEmojiPicker from "../emoji/PositionedEmojiPicker";
import type { JSX } from "solid-js";

const tick = () => new Promise((r) => setTimeout(r, 0));

/**
 * The modal contract (A11Y-1, NAV-11): a named dialog takes focus on open,
 * Tab and Shift+Tab stay inside, Escape closes it even when focus has slipped
 * behind it (body), and focus goes back to the opener when it closes.
 */
async function expectModalContract(
  name: string | RegExp,
  mount: (onClose: () => void) => JSX.Element,
) {
  document.body.innerHTML = "";
  const opener = document.createElement("button");
  opener.textContent = "opener";
  document.body.appendChild(opener);
  opener.focus();
  const onClose = vi.fn();
  const view = render(() => mount(onClose));
  await tick();

  const dialog = screen.getByRole("dialog", { name });
  expect(dialog.getAttribute("aria-modal")).toBe("true");
  expect(dialog.contains(document.activeElement)).toBe(true);

  // Tab past the last focusable element wraps to the first, and back.
  const focusables = Array.from(
    dialog.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), textarea, a[href], [tabindex]:not([tabindex='-1'])"),
  );
  const last = focusables[focusables.length - 1];
  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(focusables[0]);
  fireEvent.keyDown(focusables[0], { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);

  // Focus behind the dialog: Escape still closes it.
  (document.activeElement as HTMLElement).blur();
  expect(document.activeElement).toBe(document.body);
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);

  view.unmount();
  expect(document.activeElement).toBe(opener);
}

describe("core modals expose role dialog with a label", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("SettingsModal", () => {
    render(() => <SettingsModal onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Settings" })).toBeTruthy();
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

describe("modal contract: focus in, Tab stays, Escape from anywhere, focus back", () => {
  it("ReportModal", async () => {
    await expectModalContract("Report message", (onClose) => (
      <ReportModal target={{ userId: "u2", username: "bob", messageId: "m1" }} onClose={onClose} />
    ));
  });

  it("BlockConfirmModal starts on Cancel", async () => {
    await expectModalContract("Block person", (onClose) => (
      <BlockConfirmModal userId="u2" username="bob" onClose={onClose} />
    ));
    document.body.innerHTML = "";
    render(() => <BlockConfirmModal userId="u2" username="bob" onClose={() => {}} />);
    await tick();
    expect(document.activeElement?.textContent).toBe("Cancel");
    expect(screen.getByRole("dialog").getAttribute("aria-describedby")).toBeTruthy();
  });

  it("ImageLightbox", async () => {
    await expectModalContract("cat.png", (onClose) => (
      <ImageLightbox src="blob:x" alt="cat.png" onClose={onClose} />
    ));
  });
});

describe("emoji picker", () => {
  it("is a named popup: Escape closes only the picker and focus goes back to its button", async () => {
    document.body.innerHTML = "";
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    anchor.focus();
    const pageEscape = vi.fn();
    document.addEventListener("keydown", pageEscape);
    const onClose = vi.fn();
    const [open, setOpen] = createSignal(true);
    render(() => (
      <Show when={open()}>
        <PositionedEmojiPicker
          anchorEl={anchor}
          onSelect={() => {}}
          onClose={() => {
            onClose();
            setOpen(false);
          }}
        />
      </Show>
    ));
    await tick();
    const picker = screen.getByRole("dialog", { name: "Choose an emoji" });
    expect(picker.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(pageEscape).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(anchor);
    document.removeEventListener("keydown", pageEscape);
  });

  it("Escape in a picker opened from a dialog leaves the dialog open", async () => {
    document.body.innerHTML = "";
    const closeDialog = vi.fn();
    const [pick, setPick] = createSignal(false);
    render(() => (
      <ReportModal target={{ userId: "u2", username: "bob" }} onClose={closeDialog} />
    ));
    await tick();
    const anchor = screen.getByRole("dialog").querySelector("button")!;
    render(() => (
      <Show when={pick()}>
        <PositionedEmojiPicker anchorEl={anchor} onSelect={() => {}} onClose={() => setPick(false)} />
      </Show>
    ));
    setPick(true);
    await tick();
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(pick()).toBe(false);
    expect(closeDialog).not.toHaveBeenCalled();
  });
});
