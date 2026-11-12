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
  listGuildEvents: vi.fn(async () => []),
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
vi.mock("@/stores/settings", () => ({ appSettings: () => null }));
vi.mock("../pages/MarkdownPreview", () => ({ default: (p: { content: string }) => <p>{p.content}</p> }));
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
vi.mock("../guilds/GeneralTab", () => ({ default: () => <div data-testid="tab-GeneralTab" /> }));
vi.mock("../guilds/InvitesTab", () => ({ default: () => <div data-testid="tab-InvitesTab" /> }));
vi.mock("../guilds/MembersTab", () => ({ default: () => <div data-testid="tab-MembersTab" /> }));
vi.mock("../guilds/RolesTab", () => ({ default: () => <div data-testid="tab-RolesTab" /> }));
vi.mock("../guilds/ReactionRolesTab", () => ({ default: () => <div data-testid="tab-ReactionRolesTab" /> }));
vi.mock("../guilds/EmojisTab", () => ({ default: () => <div data-testid="tab-EmojisTab" /> }));
vi.mock("../guilds/BotsTab", () => ({ default: () => <div data-testid="tab-BotsTab" /> }));
vi.mock("../guilds/WebhooksTab", () => ({ default: () => <div data-testid="tab-WebhooksTab" /> }));
vi.mock("../guilds/SafetyTab", () => ({ default: () => <div data-testid="tab-SafetyTab" /> }));
vi.mock("../guilds/UsageTab", () => ({ default: () => <div data-testid="tab-UsageTab" /> }));
vi.mock("../guilds/CategoriesTab", () => ({ default: () => <div data-testid="tab-CategoriesTab" /> }));
vi.mock("../guilds/RoleEditor", () => ({ default: () => <div data-testid="tab-RoleEditor" /> }));
vi.mock("@/stores/guildAdmin", () => ({ deleteServer: vi.fn(), leaveServer: vi.fn(), canManageChannel: () => true }));

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
  guildsState: { guilds: [{ id: "g1", name: "Familie", icon_url: null, owner_id: "x" }] },
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
import Modal from "../ui/Modal";
import Avatar from "../ui/Avatar";
import StatusIndicator from "../ui/StatusIndicator";
import EventsModal from "../guilds/EventsModal";
import PageAcceptanceModal from "../pages/PageAcceptanceModal";
import CategoryManager from "../pages/CategoryManager";
import CustomStatusModal from "../ui/CustomStatusModal";
import KeyboardShortcutsDialog from "../ui/KeyboardShortcutsDialog";
import PositionedEmojiPicker from "../emoji/PositionedEmojiPicker";
import GuildSettingsModal from "../guilds/GuildSettingsModal";
import CreateGuildModal from "../guilds/CreateGuildModal";
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
    expect(screen.getByRole("dialog", { name: "Create channel" })).toBeTruthy();
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

  it("CreateGuildModal", async () => {
    await expectModalContract("Create a server", (onClose) => <CreateGuildModal onClose={onClose} />);
  });

  it("CreateChannelModal", async () => {
    await expectModalContract("Create channel", (onClose) => (
      <CreateChannelModal guildId="g1" onClose={onClose} />
    ));
  });

  it("ChannelSettingsModal", async () => {
    await expectModalContract("general", (onClose) => (
      <ChannelSettingsModal channelId="c1" guildId="g1" onClose={onClose} />
    ));
  });

  it("EventsModal", async () => {
    await expectModalContract("Events", (onClose) => (
      <EventsModal guildId="g1" canManage onClose={onClose} />
    ));
  });

  it("CustomStatusModal", async () => {
    await expectModalContract("Set Custom Status", (onClose) => (
      <CustomStatusModal onSave={() => {}} onClose={onClose} />
    ));
  });

  it("KeyboardShortcutsDialog", async () => {
    await expectModalContract("Keyboard shortcuts", (onClose) => (
      <KeyboardShortcutsDialog onClose={onClose} />
    ));
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

describe("settings windows", () => {
  it("server settings: Escape closes even when focus is still on the gear behind it", async () => {
    await expectModalContract("Familie", (onClose) => (
      <GuildSettingsModal guildId="g1" onClose={onClose} />
    ));
  });

  it("settings window keeps the modal contract", async () => {
    await expectModalContract("Settings", (onClose) => <SettingsModal onClose={onClose} />);
  });
});

describe("tabs (KONTO-10)", () => {
  it("settings: arrow keys move between tabs, the open one is selected and names the panel", () => {
    document.body.innerHTML = "";
    render(() => <SettingsModal onClose={() => {}} />);
    const list = screen.getByRole("tablist", { name: "Settings" });
    const account = screen.getByRole("tab", { name: "My Account" });
    expect(account.getAttribute("aria-selected")).toBe("true");
    expect(account.tabIndex).toBe(0);
    expect(screen.getByRole("tab", { name: "Appearance" }).tabIndex).toBe(-1);

    account.focus();
    fireEvent.keyDown(account, { key: "ArrowDown" });
    const appearance = screen.getByRole("tab", { name: "Appearance" });
    expect(appearance.getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe(appearance);
    expect(screen.getByRole("tabpanel", { name: "Appearance" })).toBeTruthy();

    fireEvent.keyDown(appearance, { key: "End" });
    expect(screen.getByRole("tab", { name: "Security" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(account.getAttribute("aria-selected")).toBe("true");
    expect(list.contains(document.activeElement)).toBe(true);
  });

  it("server settings: tabs with arrow keys, advanced tabs say so", () => {
    document.body.innerHTML = "";
    render(() => <GuildSettingsModal guildId="g1" onClose={() => {}} />);
    screen.getByRole("tablist", { name: "Server settings" });
    const members = screen.getByRole("tab", { name: "Members" });
    expect(members.getAttribute("aria-selected")).toBe("true");
    members.focus();
    fireEvent.keyDown(members, { key: "ArrowDown" });
    const usage = screen.getByRole("tab", { name: "Usage" });
    expect(usage.getAttribute("aria-selected")).toBe("true");
    expect(usage.getAttribute("aria-describedby")).toBeTruthy();
    expect(screen.getByRole("tabpanel", { name: "Usage" })).toBeTruthy();
  });
});

describe("nested dialogs", () => {
  it("Escape closes only the top dialog, then the one below; each returns focus to its opener", async () => {
    document.body.innerHTML = "";
    const [outer, setOuter] = createSignal(true);
    const [inner, setInner] = createSignal(false);
    render(() => (
      <Show when={outer()}>
        <Modal onClose={() => setOuter(false)} label="Outer">
          <button onClick={() => setInner(true)}>open inner</button>
          <Show when={inner()}>
            <Modal onClose={() => setInner(false)} label="Inner">
              <button>inner action</button>
            </Modal>
          </Show>
        </Modal>
      </Show>
    ));
    await tick();
    const openInner = screen.getByRole("button", { name: "open inner" });
    expect(document.activeElement).toBe(openInner);
    fireEvent.click(openInner);
    await tick();
    const innerDialog = screen.getByRole("dialog", { name: "Inner" });
    expect(innerDialog.contains(document.activeElement)).toBe(true);
    // The newer dialog is stacked above the older one.
    const z = (name: string) =>
      Number((screen.getByRole("dialog", { name }).parentElement as HTMLElement).style.zIndex);
    expect(z("Inner")).toBeGreaterThan(z("Outer"));

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Inner" })).toBeNull();
    expect(screen.getByRole("dialog", { name: "Outer" })).toBeTruthy();
    expect(document.activeElement).toBe(openInner);

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Outer" })).toBeNull();
  });

  it("a click on the backdrop closes, a click inside does not", async () => {
    document.body.innerHTML = "";
    const onClose = vi.fn();
    render(() => (
      <Modal onClose={onClose} label="Box">
        <p>text</p>
      </Modal>
    ));
    await tick();
    const dialog = screen.getByRole("dialog", { name: "Box" });
    fireEvent.click(dialog);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(dialog.parentElement!);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("pages", () => {
  const page = { id: "p1", title: "House rules", content: "Be kind.", guild_id: "g1" } as never;

  it("PageAcceptanceModal: closable when not required", async () => {
    await expectModalContract("House rules", (onClose) => (
      <PageAcceptanceModal page={page} onAccept={async () => {}} onClose={onClose} />
    ));
  });

  it("PageAcceptanceModal: required pages do not close with Escape; the text is reachable by keyboard", async () => {
    document.body.innerHTML = "";
    const onClose = vi.fn();
    render(() => (
      <PageAcceptanceModal page={page} isBlocking onAccept={async () => {}} onClose={onClose} />
    ));
    await tick();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "House rules" })).toBeTruthy();
    // Keyboard users scroll the text themselves to reach "I accept".
    expect(screen.getByRole("region", { name: "House rules" }).tabIndex).toBe(0);
  });

  it("CategoryManager: Escape first cancels a rename, then closes", async () => {
    document.body.innerHTML = "";
    const onClose = vi.fn();
    render(() => (
      <CategoryManager
        guildId="g1"
        categories={[{ id: "c1", name: "Rules" } as never]}
        onCreateCategory={async () => {}}
        onUpdateCategory={async () => {}}
        onDeleteCategory={async () => {}}
        onClose={onClose}
      />
    ));
    await tick();
    screen.getByRole("dialog", { name: "Page categories" });
    fireEvent.click(screen.getByRole("button", { name: "Rename Rules" }));
    expect(screen.getByRole("textbox", { name: "Category name" })).toBeTruthy();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Category name" })).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("states are not colour only (A11Y-14)", () => {
  it("the online status next to an avatar has a word for screen readers", () => {
    document.body.innerHTML = "";
    render(() => <Avatar alt="Oma Erna" status="dnd" showStatus />);
    expect(screen.getByRole("img", { name: "Do Not Disturb" })).toBeTruthy();
    // Without a picture the initials stand for the person, not for "OE".
    expect(screen.getByRole("img", { name: "Oma Erna" })).toBeTruthy();
  });

  it("is silent where the status is already written next to it", () => {
    document.body.innerHTML = "";
    render(() => <StatusIndicator status="online" decorative />);
    expect(screen.queryByRole("img")).toBeNull();
  });
});
