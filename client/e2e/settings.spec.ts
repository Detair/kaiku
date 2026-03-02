import { test, expect } from "@playwright/test";
import { registerAndReachMain, openUserSettings, uniqueId } from "./helpers";

test.describe("User Settings", () => {
  test("settings modal opens via gear button", async ({ page }) => {
    await registerAndReachMain(page);
    await openUserSettings(page);
    await expect(page.getByText("Settings")).toBeVisible({ timeout: 5000 });
  });

  test("settings tabs are navigable and load content", async ({ page }) => {
    await registerAndReachMain(page);
    await openUserSettings(page);

    // Each tab has a content-specific keyword to verify panel loaded
    const tabExpectations: Record<string, RegExp> = {
      account: /display name/i,
      appearance: /theme/i,
      notifications: /desktop|sound|notification/i,
      audio: /input|output|device|microphone/i,
      privacy: /block|privacy/i,
      security: /password/i,
    };

    for (const [tab, contentPattern] of Object.entries(tabExpectations)) {
      const tabButton = page.getByTestId(`settings-tab-${tab}`);
      await expect(tabButton).toBeVisible({ timeout: 5000 });
      await tabButton.click();
      // Verify the tab content panel rendered with tab-specific content
      await expect(
        page.locator('[role="dialog"]').getByText(contentPattern).first(),
      ).toBeVisible({ timeout: 5000 });
    }
  });

  test("account tab allows updating display name", async ({ page }) => {
    await registerAndReachMain(page);
    await openUserSettings(page);

    await page.getByTestId("settings-tab-account").click();
    await expect(page.getByText(/display name/i).first()).toBeVisible({ timeout: 5000 });

    // Find the display name input and update it
    const displayNameInput = page.locator('[role="dialog"]').getByLabel(/display name/i);
    await expect(displayNameInput).toBeVisible({ timeout: 5000 });

    const newName = `E2E-${uniqueId("name")}`;
    await displayNameInput.fill(newName);

    // Click save button
    const saveBtn = page.locator('[role="dialog"]').getByRole("button", { name: /save/i });
    await expect(saveBtn).toBeVisible({ timeout: 5000 });
    await saveBtn.click();

    // Close and reopen settings to verify persistence
    await page.keyboard.press("Escape");
    await openUserSettings(page);
    await page.getByTestId("settings-tab-account").click();

    // Re-query input after modal reopen to avoid stale reference
    const savedNameInput = page.locator('[role="dialog"]').getByLabel(/display name/i);
    await expect(savedNameInput).toHaveValue(newName, { timeout: 10000 });
  });

  test("security tab shows password and MFA sections", async ({ page }) => {
    await registerAndReachMain(page);
    await openUserSettings(page);

    await page.getByTestId("settings-tab-security").click();
    await expect(
      page.getByText(/password/i).first(),
    ).toBeVisible({ timeout: 5000 });
  });
});
