/**
 * Admin Dashboard E2E Tests
 *
 * Tests admin panel access, navigation, and basic functionality.
 * Prerequisites: Backend running, admin user created (admin/admin123)
 */

import { test, expect } from "@playwright/test";
import { loginAsAdmin, loginAsAlice, navigateToAdmin, uniqueId } from "./helpers";

test.describe("Admin Dashboard", () => {
  test("should access admin dashboard", async ({ page }) => {
    await loginAsAdmin(page);
    await navigateToAdmin(page);
  });

  test("should display admin panels", async ({ page }) => {
    await loginAsAdmin(page);
    await navigateToAdmin(page);
    await expect(page.getByRole("button", { name: "Users", exact: true })).toBeVisible({
      timeout: 5000,
    });
  });

  test("should show users panel", async ({ page }) => {
    await loginAsAdmin(page);
    await navigateToAdmin(page);

    const usersBtn = page.getByRole("button", { name: "Users", exact: true });
    await expect(usersBtn).toBeVisible({ timeout: 3000 });
    await usersBtn.click();
    await expect(page.getByRole("heading", { name: "Users" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("should show guilds panel", async ({ page }) => {
    await loginAsAdmin(page);
    await navigateToAdmin(page);

    const guildsBtn = page.getByRole("button", { name: "Guilds", exact: true });
    await expect(guildsBtn).toBeVisible({ timeout: 3000 });
    await guildsBtn.click();
    await expect(page.getByRole("heading", { name: "Guilds" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("should show audit log panel", async ({ page }) => {
    await loginAsAdmin(page);
    await navigateToAdmin(page);

    const auditBtn = page.getByRole("button", { name: "Audit Log", exact: true });
    await expect(auditBtn).toBeVisible({ timeout: 3000 });
    await auditBtn.click();
    await expect(page.getByRole("heading", { name: "Audit Log" })).toBeVisible({
      timeout: 5000,
    });
  });

  test("should block non-admin access", async ({ page }) => {
    await loginAsAlice(page);
    await page.goto("/admin");

    // Non-admin should be redirected away or shown a forbidden message
    await expect(async () => {
      const isOnAdmin = page.url().includes("/admin");
      const hasForbidden = await page
        .locator('text=Forbidden')
        .or(page.locator('text=Access Denied'))
        .or(page.locator('text=not authorized'))
        .isVisible();
      expect(!isOnAdmin || hasForbidden).toBeTruthy();
    }).toPass({ timeout: 5000 });
  });
});

test.describe("Admin-created accounts", () => {
  test("admin creates an account and the new user must change the password", async ({
    page,
  }) => {
    const username = uniqueId("fpc").replace(/-/g, "_").toLowerCase().slice(0, 32);
    const temporaryPassword = "Temp-pass-1234";
    const newPassword = "Own-pass-56789";

    // Admin: elevate, create the account via the modal, sign out.
    await loginAsAdmin(page);
    await navigateToAdmin(page);
    await page.getByRole("button", { name: "Not Elevated" }).click();
    await page.getByRole("button", { name: "Elevate Session" }).click();
    await expect(page.getByText("Session Elevated")).toBeVisible({ timeout: 10000 });
    await page.keyboard.press("Escape");

    await page.getByRole("button", { name: "Users", exact: true }).click();
    await page.getByRole("button", { name: "Create account" }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("Username").fill(username);
    await dialog.getByLabel("Temporary password").fill(temporaryPassword);
    await dialog.getByRole("button", { name: "Create account" }).click();
    await expect(dialog.getByText("Account created")).toBeVisible({ timeout: 10000 });
    await dialog.getByRole("button", { name: "Close" }).first().click();

    await page.goto("/");
    await page.getByTestId("logout-button").click();
    await expect(page).toHaveURL(/\/login/, { timeout: 10000 });

    // New user: temporary password → change screen → own password → app.
    await page.fill('input[placeholder="Enter your username"]', username);
    await page.fill('input[placeholder="Enter your password"]', temporaryPassword);
    await page.click('button[type="submit"]');
    await expect(
      page.getByRole("heading", { name: "Choose your own password" }),
    ).toBeVisible({ timeout: 15000 });
    // The temporary password from this sign-in is reused, not asked again.
    await expect(page.getByLabel("Temporary password")).toHaveCount(0);

    await page.getByLabel("New password", { exact: true }).fill(newPassword);
    await page.getByLabel("Confirm new password").fill(newPassword);
    await page.getByRole("button", { name: "Save password" }).click();
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await expect(
      page.getByRole("heading", { name: "Choose your own password" }),
    ).toHaveCount(0);

    // The temporary password no longer works.
    await page.getByTestId("logout-button").click();
    await expect(page).toHaveURL(/\/login/, { timeout: 10000 });
    await page.fill('input[placeholder="Enter your username"]', username);
    await page.fill('input[placeholder="Enter your password"]', temporaryPassword);
    await page.click('button[type="submit"]');
    await expect(page.getByRole("alert").first()).toBeVisible({ timeout: 10000 });
    await expect(page).toHaveURL(/\/login/);
  });
});
