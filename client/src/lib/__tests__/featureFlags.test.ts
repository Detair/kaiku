import { afterEach, describe, expect, it, vi } from "vitest";
import { isE2EEEnabled } from "../featureFlags";

describe("featureFlags", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("isE2EEEnabled is false by default", () => {
    vi.stubEnv("VITE_FEATURE_E2EE", "");
    expect(isE2EEEnabled()).toBe(false);
  });

  it('isE2EEEnabled is true only for the exact string "true"', () => {
    vi.stubEnv("VITE_FEATURE_E2EE", "true");
    expect(isE2EEEnabled()).toBe(true);
    for (const v of ["TRUE", "1", "yes", "false", " true"]) {
      vi.stubEnv("VITE_FEATURE_E2EE", v);
      expect(isE2EEEnabled()).toBe(false);
    }
  });
});
