/**
 * Shared E2E Test Helpers
 *
 * Common utilities for Playwright tests. All tests should use these
 * helpers instead of duplicating login/navigation logic.
 */

import { expect, type Locator, type Page } from "@playwright/test";

interface RegisterAndReachMainOptions {
  usernamePrefix?: string;
  setupServerName?: string;
}

async function waitForOptionalVisible(locator: Locator, timeout: number): Promise<boolean> {
  try {
    await locator.waitFor({ state: "visible", timeout });
    return true;
  } catch (error) {
    console.log(
      `[waitForOptionalVisible] Element not visible within ${timeout}ms, skipping. Error: ${error instanceof Error ? error.message : String(error)}`
    );
    return false;
  }
}

/** Login as a specific user and wait for the app to load. */
export async function login(
  page: Page,
  username: string,
  password: string = "password123"
) {
  await page.goto("/login");
  await page.getByTestId("login-username").fill(username);
  await page.getByTestId("login-password").fill(password);
  await page.getByTestId("login-submit").click();
  // Wait for sidebar (indicates successful login + app loaded)
  await expect(page.locator("aside")).toBeVisible({ timeout: 15000 });
}

/** Open the user settings modal via the gear icon in UserPanel. */
export async function openUserSettings(page: Page) {
  await page.getByTestId("user-settings-button").click();
  await expect(page.locator('[role="dialog"], .fixed.inset-0')).toBeVisible({
    timeout: 5000,
  });
}

export async function registerAndReachMain(
  page: Page,
  options: RegisterAndReachMainOptions = {}
) {
  const username = uniqueUsername(options.usernamePrefix ?? "e2e");
  const setupServerName = options.setupServerName ?? "E2E Server";

  await page.goto("/register");
  await page.getByTestId("register-server-url").fill("http://localhost:8080");
  await page.getByTestId("register-username").fill(username);
  await page.getByTestId("register-password").fill("password123");
  await page.getByTestId("register-password-confirm").fill("password123");
  await page.getByTestId("register-submit").click();

  const setupWizard = page.getByTestId("setup-wizard");
  if (await waitForOptionalVisible(setupWizard, 15000)) {
    await setupWizard.getByTestId("setup-server-name").fill(setupServerName);
    await setupWizard.getByTestId("setup-complete").click();
  }

  const onboardingWizard = page.getByTestId("onboarding-wizard");
  if (await waitForOptionalVisible(onboardingWizard, 5000)) {
    const nextButton = onboardingWizard.getByTestId("onboarding-next");
    await expect(nextButton).toBeVisible({ timeout: 10000 });
    await expect(nextButton).toBeEnabled({ timeout: 10000 });
    await nextButton.click();

    // Skip remaining steps until "Get Started" appears
    const skipButton = onboardingWizard.getByTestId("onboarding-skip");
    const getStartedButton = onboardingWizard.getByTestId("onboarding-get-started");
    while (await skipButton.isVisible()) {
      if (await getStartedButton.isVisible()) break;
      await skipButton.click();
      await page.waitForTimeout(200);
    }

    await expect(getStartedButton).toBeVisible({ timeout: 10000 });
    await expect(getStartedButton).toBeEnabled({ timeout: 10000 });
    await getStartedButton.click();
    await expect(onboardingWizard).toBeHidden({ timeout: 15000 });
  }

  await expect(page.getByTestId("user-settings-button")).toBeVisible({ timeout: 15000 });

  return { username };
}

/** Navigate to home view by clicking the Home button. */
export async function goHome(page: Page) {
  await page.getByTestId("home-button").click();
}

/** Open the global search panel (button or keyboard shortcut). */
export async function openSearch(page: Page) {
  const searchBtn = page.getByTestId("search-input");
  if (await searchBtn.isVisible()) {
    // Search panel already open
    return;
  }
  // Try keyboard shortcut to open search
  await page.keyboard.press("Control+Shift+f");
  await expect(page.getByTestId("search-input")).toBeVisible({ timeout: 5000 });
}

/** Generate a unique string for test data to avoid collisions. */
export function uniqueId(prefix: string = "e2e"): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
}

export function uniqueUsername(prefix: string = "e2e"): string {
  const suffix = Math.random().toString(36).slice(2, 6);
  return `${prefix}${Date.now()}${suffix}`.toLowerCase().replace(/[^a-z0-9_]/g, "");
}

/** Create a guild via the UI. Returns the guild name used. */
export async function createGuild(page: Page, name?: string): Promise<string> {
  const guildName = name ?? uniqueId("Guild");
  await page.getByTestId("create-server-button").click();
  const nameInput = page.getByTestId("create-guild-name");
  await expect(nameInput).toBeVisible({ timeout: 10000 });
  await nameInput.fill(guildName);
  await page.getByTestId("create-guild-submit").click();
  await expect(page.getByTestId("guild-button").filter({ hasText: guildName })).toBeVisible({
    timeout: 15000,
  });
  return guildName;
}

/** Create a text channel in the current guild. Returns the channel name used. */
export async function createTextChannel(page: Page, name?: string): Promise<string> {
  const channelName = name ?? uniqueId("channel");
  await page.getByTestId("create-channel-button").first().click();
  const nameInput = page.getByTestId("create-channel-name");
  await expect(nameInput).toBeVisible({ timeout: 10000 });
  await nameInput.fill(channelName);
  await page.getByTestId("create-channel-submit").click();
  await expect(page.getByText(channelName).first()).toBeVisible({ timeout: 10000 });
  return channelName;
}

/** Select a channel by name in the sidebar. */
export async function selectChannel(page: Page, name: string) {
  await page.getByText(name).first().click();
  await expect(page.getByTestId("message-input")).toBeVisible({ timeout: 10000 });
}

/** Send a message in the current channel and wait for it to appear. */
export async function sendMessage(page: Page, text: string) {
  const input = page.getByTestId("message-input");
  await expect(input).toBeVisible({ timeout: 10000 });
  await input.fill(text);
  await input.press("Enter");
  await expect(page.getByText(text)).toBeVisible({ timeout: 15000 });
}

/** Open guild settings via the settings button in the sidebar header. */
export async function openGuildSettings(page: Page) {
  await page.getByTestId("guild-settings-button").click();
  await expect(page.locator('[role="dialog"]')).toBeVisible({ timeout: 5000 });
}

/** Ensure a guild exists and is selected; creates one if none exist. Returns guild name. */
export async function ensureGuildSelected(page: Page): Promise<string> {
  const guildButtons = page.getByTestId("guild-button");
  if ((await guildButtons.count()) === 0) {
    return await createGuild(page);
  }
  const firstButton = guildButtons.first();
  await firstButton.click();
  return (await firstButton.textContent()) ?? "";
}
