/**
 * Shared E2E Test Helpers
 *
 * Common utilities for Playwright tests. All tests should use these
 * helpers instead of duplicating login/navigation logic.
 *
 * Test users (created via scripts/create-test-users.sh):
 *   admin / admin123  — Guild owner, system admin
 *   alice / password123 — Regular member
 *   bob   / password123 — Regular member
 */

import { expect, type Page } from "@playwright/test";

async function completeFirstRunSetupIfVisible(page: Page) {
  const setupHeading = page.getByRole("heading", { name: /Welcome to Kaiku/i });
  const setupVisible = await setupHeading.isVisible({ timeout: 1000 }).catch(() => false);

  if (!setupVisible) {
    return;
  }

  const completeButton = page.locator('button:has-text("Complete Setup")').first();
  await expect(completeButton).toBeVisible({ timeout: 5000 });
  await completeButton.click();
  await expect(setupHeading).toBeHidden({ timeout: 15000 });
}

async function completeOnboardingIfVisible(page: Page) {
  const onboardingDialog = page.getByRole("dialog", { name: "Onboarding wizard" });
  const onboardingVisible = await onboardingDialog
    .isVisible({ timeout: 1000 })
    .catch(() => false);

  if (!onboardingVisible) {
    return;
  }

  for (let step = 0; step < 6; step += 1) {
    const getStarted = onboardingDialog.getByRole("button", { name: "Get Started" });
    if (await getStarted.isVisible({ timeout: 300 }).catch(() => false)) {
      await getStarted.click();
      break;
    }

    const continueButton = onboardingDialog.getByRole("button", { name: "Continue" });
    if (await continueButton.isVisible({ timeout: 300 }).catch(() => false)) {
      await continueButton.click();
      continue;
    }

    const nextButton = onboardingDialog.getByRole("button", { name: "Next" });
    if (await nextButton.isVisible({ timeout: 300 }).catch(() => false)) {
      await nextButton.click();
      continue;
    }

    const skipButton = onboardingDialog.getByRole("button", { name: "Skip" });
    if (await skipButton.isVisible({ timeout: 300 }).catch(() => false)) {
      await skipButton.click();
      continue;
    }
  }

  await expect(onboardingDialog).toBeHidden({ timeout: 15000 });
}

/** Login as a specific user and wait for the app to load. */
export async function login(
  page: Page,
  username: string,
  password: string = "password123"
) {
  await page.goto("/login");
  await page.fill('input[placeholder="Enter your username"]', username);
  await page.fill('input[placeholder="Enter your password"]', password);
  await page.click('button[type="submit"]');
  await completeFirstRunSetupIfVisible(page);
  await completeOnboardingIfVisible(page);
  // Wait for sidebar (indicates successful login + app loaded)
  await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
}

/** Login as admin (owner, system admin). */
export async function loginAsAdmin(page: Page) {
  await login(page, "admin", "admin123");
}

/** Login as alice (regular member). */
export async function loginAsAlice(page: Page) {
  await login(page, "alice");
}

/** Click the first guild in the server rail. */
export async function selectFirstGuild(page: Page) {
  const guildButtons = page.locator("aside").first().locator(
    'button[title]:not([title="Home"]):not([title="Explore Servers"]):not([title="Create Server"]):not([title="Join Server"])',
  );
  const firstGuild = guildButtons.first();
  await expect(firstGuild).toBeVisible({ timeout: 10000 });
  await firstGuild.click();
}

export async function selectGuildByName(page: Page, guildName: string) {
  const guildButton = page.locator(`button[title="${guildName}"]`).first();
  if (await guildButton.isVisible({ timeout: 5000 }).catch(() => false)) {
    await guildButton.click();
    return;
  }

  await selectFirstGuild(page);
}

/** Click the first text channel in the channel list. */
export async function selectFirstChannel(page: Page) {
  // Wait for channel list and click first channel item
  const channel = page
    .locator("aside")
    .nth(1)
    .locator('[role="button"]')
    .filter({ hasText: /#|general/ })
    .first();
  await expect(channel).toBeVisible({ timeout: 10000 });
  await channel.click();
}

/** Open the user settings modal via the gear icon in UserPanel. */
export async function openUserSettings(page: Page) {
  await page.click('button[title="User Settings"]');
  await expect(page.getByRole("dialog").first()).toBeVisible({
    timeout: 5000,
  });
}

/** Navigate to home view by clicking the Home button. */
export async function goHome(page: Page) {
  await page.click('button[title="Home"]');
}

/** Open the global search panel (button or keyboard shortcut). */
export async function openSearch(page: Page) {
  const searchBtn = page.locator('button:has-text("Search")');
  if (await searchBtn.isVisible({ timeout: 2000 })) {
    await searchBtn.click();
  } else {
    await page.keyboard.press("Control+Shift+f");
  }
  await expect(
    page.locator('input[placeholder*="search" i], input[type="search"]')
  ).toBeVisible({ timeout: 5000 });
}

/** Navigate to the admin dashboard and wait for it to load. */
export async function navigateToAdmin(page: Page) {
  await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });

  const attempts = 3;
  let lastError: unknown = null;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      await page.goto("/admin", { waitUntil: "domcontentloaded" });
      await expect(page.getByRole("heading", { name: "Admin Dashboard" })).toBeVisible({
        timeout: 10000,
      });
      return;
    } catch (error) {
      lastError = error;
      const message = error instanceof Error ? error.message : String(error);
      const isRetryable =
        message.includes("net::ERR_TOO_MANY_RETRIES") || message.includes("Navigation timeout");

      if (!isRetryable || attempt === attempts) {
        break;
      }

      await page.waitForTimeout(500 * attempt);
    }
  }

  throw lastError instanceof Error
    ? lastError
    : new Error("Failed to navigate to admin dashboard");
}

/** Generate a unique string for test data to avoid collisions. */
export function uniqueId(prefix: string = "e2e"): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
}
