import { test, expect } from "@playwright/test";

test.describe("Initial setup and onboarding", () => {
  test.describe.configure({ mode: "serial" });

  test("first user can complete setup and onboarding", async ({ page }) => {
    const username = `firstuser${Date.now()}`;

    await page.goto("/register");
    await page.getByTestId("register-server-url").fill("http://localhost:8080");
    await page.getByTestId("register-username").fill(username);
    await page.getByTestId("register-password").fill("password123");
    await page.getByTestId("register-password-confirm").fill("password123");
    await page.getByTestId("register-submit").click();

    // First user should always see the setup wizard
    const setupWizard = page.getByTestId("setup-wizard");
    await expect(setupWizard).toBeVisible({ timeout: 15000 });
    await page.getByTestId("setup-server-name").fill("E2E Initial Server");
    await page.getByTestId("setup-complete").click();

    const onboardingDialog = page.getByTestId("onboarding-wizard");
    await expect(onboardingDialog).toBeVisible({ timeout: 20000 });

    await page.getByTestId("onboarding-display-name").fill("First User");
    await onboardingDialog.getByTestId("onboarding-next").click();

    // Skip remaining steps until "Get Started" appears
    const skipButton = onboardingDialog.getByTestId("onboarding-skip");
    const getStartedButton = onboardingDialog.getByTestId("onboarding-get-started");
    while (await skipButton.isVisible()) {
      if (await getStartedButton.isVisible()) break;
      await skipButton.click();
      await page.waitForTimeout(200);
    }

    await expect(getStartedButton).toBeVisible({ timeout: 10000 });
    await getStartedButton.click();

    await expect(onboardingDialog).toBeHidden({ timeout: 10000 });
    await expect(page.getByTestId("user-settings-button")).toBeVisible({
      timeout: 10000,
    });
  });
});
