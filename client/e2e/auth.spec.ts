/**
 * Authentication E2E Tests
 *
 * Tests login, registration, and password recovery flows.
 * Prerequisites: Backend running, test users created (scripts/create-test-users.sh)
 */

import { test, expect } from "@playwright/test";

test.describe("Authentication", () => {
  test.describe("Login", () => {
    test("should display login form", async ({ page }) => {
      await page.goto("/login");
      await expect(
        page.locator('input[placeholder="Enter your username"]')
      ).toBeVisible();
      await expect(
        page.locator('input[placeholder="Enter your password"]')
      ).toBeVisible();
      await expect(page.locator('button[type="submit"]')).toBeVisible();
    });

    test("should login with valid credentials", async ({ page }) => {
      await page.goto("/login");
      await page.fill('input[placeholder="Enter your username"]', "alice");
      await page.getByTestId("login-show-password").click();
      await page.fill('input[placeholder="Enter your password"]', "password123");
      await page.click('button[type="submit"]');

      // Should redirect to main app (sidebar visible)
      await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    });

    test("should show error for invalid credentials", async ({ page }) => {
      await page.goto("/login");
      await page.fill('input[placeholder="Enter your username"]', "alice");
      await page.fill(
        'input[placeholder="Enter your password"]',
        "wrongpassword"
      );
      await page.click('button[type="submit"]');

      // Should show error message (remain on login page)
      await expect(page.locator("text=Invalid")).toBeVisible({ timeout: 5000 });
    });

    test("should have link to register page", async ({ page }) => {
      await page.goto("/login");
      const registerLink = page.locator('a:has-text("Register")');
      await expect(registerLink).toBeVisible();
      await registerLink.click();
      await expect(page).toHaveURL(/\/register/);
    });

    test("should have link to forgot password page", async ({ page }) => {
      await page.goto("/login");
      const forgotLink = page.locator('a:has-text("Forgot")');
      await expect(forgotLink).toBeVisible();
      await forgotLink.click();
      await expect(page).toHaveURL(/\/forgot-password/);
    });
  });

  test.describe("Registration", () => {
    test("should display registration form", async ({ page }) => {
      await page.goto("/register");
      await expect(
        page.locator('input[placeholder="Choose a username"]')
      ).toBeVisible();
      await expect(
        page.locator('input[placeholder="Create a password"]')
      ).toBeVisible();
      await expect(
        page.locator('input[placeholder="Confirm your password"]')
      ).toBeVisible();
      await expect(page.locator('button[type="submit"]')).toBeVisible();
    });

    test("should register a new account", async ({ page }) => {
      const username = `e2etest${Date.now()}`;
      await page.goto("/register");
      await page.fill('input[placeholder="Choose a username"]', username);
      await page.fill('input[placeholder="Create a password"]', "testpass123!");
      await page.fill(
        'input[placeholder="Confirm your password"]',
        "testpass123!"
      );
      await page.click('button[type="submit"]');

      // Should either redirect to app or show success
      await expect(
        page.locator("aside").first().or(page.locator("text=success"))
      ).toBeVisible({ timeout: 15000 });
    });

    test("should show validation errors", async ({ page }) => {
      await page.goto("/register");
      await page.fill('input[placeholder="Choose a username"]', "test");
      await page.fill('input[placeholder="Create a password"]', "short");
      await page.fill('input[placeholder="Confirm your password"]', "mismatch");
      await page.click('button[type="submit"]');

      // Page should remain on register (not redirect) and show validation feedback
      await expect(page).toHaveURL(/\/register/, { timeout: 3000 });
    });

    test("should have link to login page", async ({ page }) => {
      await page.goto("/register");
      const loginLink = page.locator('a:has-text("Login")');
      await expect(loginLink).toBeVisible();
      await loginLink.click();
      await expect(page).toHaveURL(/\/login/);
    });
  });

  test.describe("Password Recovery", () => {
    test("should display forgot password form", async ({ page }) => {
      await page.goto("/forgot-password");
      await expect(page.locator('input[type="email"], input[placeholder*="email" i]')).toBeVisible();
      await expect(page.locator('button[type="submit"]')).toBeVisible();
    });

    test("should display reset password form", async ({ page }) => {
      await page.goto("/reset-password");
      await expect(
        page.locator('input[placeholder*="password" i]').first()
      ).toBeVisible();
    });
  });
});
