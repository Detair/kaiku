/**
 * Friends & DMs E2E Tests
 *
 * Tests friends list, friend requests, and DM conversations.
 * Prerequisites: Backend running, test users created
 */

import { test, expect } from "@playwright/test";
import { loginAsAlice, goHome } from "./helpers";

test.describe("Friends & DMs", () => {
  test.beforeEach(async ({ page }) => {
    await loginAsAlice(page);
    await goHome(page);
  });

  test("should display friends list", async ({ page }) => {
    await expect(page.getByRole("button", { name: "Online" }).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("should switch between tabs", async ({ page }) => {
    const tabs = ["Online", "All", "Blocked"];
    for (const tab of tabs) {
      const tabBtn = page.getByRole("button", { name: tab, exact: true }).first();
      await expect(tabBtn).toBeVisible({ timeout: 2000 });
      await tabBtn.click();
    }
  });

  test("should show add friend form", async ({ page }) => {
    const addBtn = page.getByTitle("Add Friend");
    await expect(addBtn).toBeVisible({ timeout: 5000 });
    await addBtn.click();

    await expect(
      page.locator('input[placeholder*="username" i]')
    ).toBeVisible({ timeout: 3000 });
  });

  test("should send a friend request", async ({ page }) => {
    const addBtn = page.getByTitle("Add Friend");
    await expect(addBtn).toBeVisible({ timeout: 5000 });
    await addBtn.click();

    const input = page.locator('input[placeholder*="username" i]');
    await expect(input).toBeVisible({ timeout: 3000 });
    await input.fill("bob");

    const sendBtn = page.getByRole("button", { name: "Send request" });
    await expect(sendBtn).toBeVisible({ timeout: 2000 });
    await sendBtn.click();
    await expect(async () => {
      const modalClosed = await page
        .getByRole("heading", { name: "Add Friend" })
        .isHidden()
        .catch(() => true);
      const feedbackVisible = await page
        .locator('text=/sent|already|success/i')
        .first()
        .isVisible()
        .catch(() => false);
      expect(modalClosed || feedbackVisible).toBeTruthy();
    }).toPass({ timeout: 5000 });
  });

  test.fixme("should open DM conversation", async ({ page }) => {
    const dmItem = page.locator('aside [role="button"]').first();
    await expect(dmItem).toBeVisible({ timeout: 3000 });
    await dmItem.click();
    await expect(page.locator('textarea[placeholder*="Message"]')).toBeVisible({ timeout: 5000 });
  });
});
