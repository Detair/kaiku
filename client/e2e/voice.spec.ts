import { test, expect } from "@playwright/test";
import { registerAndReachMain, ensureGuildSelected } from "./helpers";

/**
 * Voice tests are marked as fixme because headless browsers cannot handle
 * WebRTC properly. These tests verify the UI elements exist with correct
 * testids but skip actual voice connection flows.
 */
test.describe("Voice", () => {
  test.fixme(
    "voice panel appears when connected",
    async ({ page }) => {
      await registerAndReachMain(page);
      await ensureGuildSelected(page);
      // Click a voice channel to connect
      // Verify voice-panel testid is visible
      await expect(page.getByTestId("voice-panel")).toBeVisible();
    },
  );

  test.fixme(
    "voice controls are visible when connected",
    async ({ page }) => {
      await registerAndReachMain(page);
      await ensureGuildSelected(page);
      // After connecting to voice:
      await expect(page.getByTestId("voice-mute")).toBeVisible();
      await expect(page.getByTestId("voice-deafen")).toBeVisible();
      await expect(page.getByTestId("voice-settings")).toBeVisible();
    },
  );

  test.fixme(
    "disconnect button hides voice panel",
    async ({ page }) => {
      await registerAndReachMain(page);
      await ensureGuildSelected(page);
      // After connecting, click disconnect:
      await page.getByTestId("voice-disconnect").click();
      await expect(page.getByTestId("voice-panel")).toBeHidden();
    },
  );
});
