// The first-run tour waits until the setup state of a restored session is known.
import { describe, it, expect, vi } from "vitest";

const gate = vi.hoisted(() => ({ known: false }));
vi.mock("@/stores/auth", () => ({
  authState: { isInitialized: true, setupRequired: false },
  setupStatusKnown: () => gate.known,
}));
vi.mock("@/stores/preferences", () => ({
  preferences: () => ({ onboarding_completed: false }),
  isInitialized: () => true,
}));

import { shouldShowOnboarding } from "../onboardingGate";

describe("shouldShowOnboarding", () => {
  it("waits while the setup state is not known yet", () => {
    gate.known = false;
    expect(shouldShowOnboarding()).toBe(false);
    gate.known = true;
    expect(shouldShowOnboarding()).toBe(true);
  });
});
