// Components render through @solidjs/testing-library with the real stores and
// i18n (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  joinDiscoverable: vi.fn(),
  discoverGuilds: vi.fn(),
  showToast: vi.fn(),
  joinViaInviteCode: vi.fn(),
  isGuildDiscoveryOff: vi.fn(() => false),
}));

vi.mock("@/lib/tauri", () => ({
  joinDiscoverable: mocks.joinDiscoverable,
  discoverGuilds: mocks.discoverGuilds,
  fetchApi: vi.fn(),
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: mocks.showToast }));
vi.mock("@/stores/guilds", () => ({
  // No server yet, so the join step is shown.
  guildsState: { guilds: [] },
  joinViaInviteCode: mocks.joinViaInviteCode,
  loadGuilds: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/stores/preferences", async (orig) => {
  const { createSignal } = await import("solid-js");
  const [prefs, setPrefs] = createSignal<Record<string, unknown>>({
    onboarding_completed: false,
  });
  return {
  ...(await orig<object>()),
  preferences: prefs,
  __setPrefs: setPrefs,
  updatePreference: vi.fn(),
  isInitialized: () => true,
  };
});
vi.mock("@/components/voice/MicTestPanel", () => ({ default: () => null }));
vi.mock("@/stores/serverFeatures", () => ({ isGuildDiscoveryOff: mocks.isGuildDiscoveryOff }));

import OnboardingWizard from "../OnboardingWizard";
import { setAuthState } from "@/stores/auth";
import * as prefsModule from "@/stores/preferences";
import { t } from "@/i18n";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function open(): void {
  setAuthState({ isInitialized: true, setupRequired: false });
  render(() => <OnboardingWizard />);
}

describe("OnboardingWizard", () => {
  it("wizard step titles render from the catalog", () => {
    open();
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe(
      t("onboarding.step.profile.title"),
    );
  });

  it("joined toast names the server", async () => {
    mocks.discoverGuilds.mockResolvedValue({
      guilds: [{ id: "g1", name: "Familie", icon_url: null, member_count: 3 }],
    });
    mocks.joinDiscoverable.mockResolvedValue({ already_member: false, guild_name: "Familie" });
    open();
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(await screen.findByRole("button", { name: "Join" }));
    await waitFor(() =>
      expect(mocks.showToast).toHaveBeenCalledWith(
        expect.objectContaining({ message: "You've joined Familie." }),
      ),
    );
  });

  it("invite field accepts a pasted link and joins with the code", async () => {
    mocks.discoverGuilds.mockResolvedValue({ guilds: [] });
    mocks.joinViaInviteCode.mockResolvedValue(undefined);
    open();
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(await screen.findByRole("tab", { name: t("onboarding.join.tabInvite") }));
    const input = screen.getByPlaceholderText(t("onboarding.join.invitePlaceholder")) as HTMLInputElement;
    expect(input.maxLength).toBe(-1);
    fireEvent.input(input, { target: { value: "https://kaiku.example/invite/AbCd1234" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(mocks.joinViaInviteCode).toHaveBeenCalledWith("AbCd1234"));
  });

  it("does not query discovery when the instance has it turned off", async () => {
    mocks.isGuildDiscoveryOff.mockReturnValue(true);
    try {
      open();
      fireEvent.click(screen.getByTestId("onboarding-next"));
      fireEvent.click(screen.getByTestId("onboarding-next"));
      fireEvent.click(screen.getByTestId("onboarding-next"));
      // C41: it opens on the invite code, not on an empty Discover tab.
      expect(await screen.findByPlaceholderText(t("onboarding.join.invitePlaceholder"))).toBeTruthy();
      expect(
        screen.getByRole("tab", { name: t("onboarding.join.tabInvite") }).getAttribute("aria-selected"),
      ).toBe("true");
      expect(screen.queryByText(t("onboarding.join.discoveryDisabled"))).toBeNull();
      expect(mocks.discoverGuilds).not.toHaveBeenCalled();
    } finally {
      mocks.isGuildDiscoveryOff.mockReturnValue(false);
    }
  });

  it("switches to the invite tab when the server says discovery is disabled", async () => {
    mocks.discoverGuilds.mockRejectedValue(new Error("DISCOVERY_DISABLED: Server discovery is not enabled"));
    open();
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    expect(await screen.findByPlaceholderText(t("onboarding.join.invitePlaceholder"))).toBeTruthy();
  });

  const setPrefs = (
    prefsModule as unknown as {
      __setPrefs: (v: Record<string, unknown>) => void;
    }
  ).__setPrefs;

  it("changing theme keeps the current wizard step and typed name", async () => {
    open();
    const input = screen.getByTestId("onboarding-display-name") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "Typed Name" } });
    fireEvent.click(screen.getByTestId("onboarding-next"));
    // Saving the name is async, so wait until the wizard has left step 0.
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 2 }).textContent).not.toBe(
        t("onboarding.step.profile.title"),
      ),
    );
    const title = screen.getByRole("heading", { level: 2 }).textContent;

    // A theme change replaces the whole preferences object.
    setPrefs({ onboarding_completed: false, theme: "cosy" });

    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe(title);
    fireEvent.click(screen.getByRole("button", { name: t("onboarding.nav.back") }));
    expect((screen.getByTestId("onboarding-display-name") as HTMLInputElement).value).toBe(
      "Typed Name",
    );
  });

  it("wizard still resets when it is shown again after being hidden", async () => {
    open();
    fireEvent.input(screen.getByTestId("onboarding-display-name"), {
      target: { value: "Typed Name" },
    });
    fireEvent.click(screen.getByTestId("onboarding-next"));
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 2 }).textContent).not.toBe(
        t("onboarding.step.profile.title"),
      ),
    );

    setPrefs({ onboarding_completed: true });
    expect(screen.queryByTestId("onboarding-next")).toBeNull();
    setPrefs({ onboarding_completed: false });

    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe(
      t("onboarding.step.profile.title"),
    );
    expect((screen.getByTestId("onboarding-display-name") as HTMLInputElement).value).toBe("");
  });
});
