import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";

const guildsState = vi.hoisted(() => ({ guilds: [] as { id: string }[] }));

vi.mock("@/lib/tauri", () => ({
  discoverGuilds: vi.fn(async () => ({ guilds: [] })),
  joinDiscoverable: vi.fn(),
  fetchApi: vi.fn(),
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/guilds", () => ({
  guildsState,
  joinViaInviteCode: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({
  authState: { isInitialized: true, setupRequired: false },
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
vi.mock("../voice/MicTestPanel", () => ({ default: () => null }));

import OnboardingWizard from "../OnboardingWizard";

const progress = () => screen.getByRole("progressbar");
const clickNext = () => fireEvent.click(screen.getByTestId("onboarding-next"));

describe("OnboardingWizard", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("onboarding skips the join step for members", () => {
    guildsState.guilds = [{ id: "g1" }];
    render(() => <OnboardingWizard />);
    expect(progress().getAttribute("aria-valuemax")).toBe("4");
    // welcome -> theme -> mic -> done (no join step)
    for (let i = 0; i < 3; i++) clickNext();
    expect(screen.queryByText("Join a Server")).toBeNull();
    expect(progress().getAttribute("aria-valuenow")).toBe("4");
  });

  it("non-members still see the join step", () => {
    guildsState.guilds = [];
    render(() => <OnboardingWizard />);
    expect(progress().getAttribute("aria-valuemax")).toBe("5");
    for (let i = 0; i < 3; i++) clickNext();
    expect(screen.getByText("Join a Server")).toBeTruthy();
  });
});
