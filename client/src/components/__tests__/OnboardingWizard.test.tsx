// Components render through @solidjs/testing-library with the real stores and
// i18n (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  joinDiscoverable: vi.fn(),
  discoverGuilds: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  joinDiscoverable: mocks.joinDiscoverable,
  discoverGuilds: mocks.discoverGuilds,
  fetchApi: vi.fn(),
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: mocks.showToast }));
vi.mock("@/stores/guilds", () => ({
  joinViaInviteCode: vi.fn(),
  loadGuilds: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/stores/preferences", async (orig) => ({
  ...(await orig<object>()),
  preferences: () => ({ onboarding_completed: false }),
  updatePreference: vi.fn(),
  isInitialized: () => true,
}));
vi.mock("@/components/voice/MicTestPanel", () => ({ default: () => null }));

import OnboardingWizard from "../OnboardingWizard";
import { setAuthState } from "@/stores/auth";
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
});
