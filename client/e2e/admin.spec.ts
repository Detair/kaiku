import { test, expect } from "@playwright/test";
import { registerAndReachMain } from "./helpers";

test.describe("Admin Dashboard", () => {
  test("first user can access admin dashboard", async ({ page }) => {
    await registerAndReachMain(page, {
      usernamePrefix: "admin",
      setupServerName: "E2E Admin Server",
    });

    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });
  });

  test("admin sidebar tabs are accessible", async ({ page }) => {
    await registerAndReachMain(page, {
      usernamePrefix: "admin",
      setupServerName: "E2E Admin Server",
    });

    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });

    // Verify admin sidebar tabs exist
    await expect(page.getByTestId("admin-tab-overview")).toBeVisible();
    await expect(page.getByTestId("admin-tab-users")).toBeVisible();
    await expect(page.getByTestId("admin-tab-guilds")).toBeVisible();
    await expect(page.getByTestId("admin-tab-audit-log")).toBeVisible();
    await expect(page.getByTestId("admin-tab-settings")).toBeVisible();
  });

  test("users panel shows registered users", async ({ page }) => {
    const { username } = await registerAndReachMain(page, {
      usernamePrefix: "admin",
      setupServerName: "E2E Admin Server",
    });

    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });

    await page.getByTestId("admin-tab-users").click();
    // Verify the current admin user appears in the panel content
    await expect(page.getByText(username)).toBeVisible({ timeout: 10000 });
  });

  test("settings panel shows auth configuration", async ({ page }) => {
    await registerAndReachMain(page, {
      usernamePrefix: "admin",
      setupServerName: "E2E Admin Server",
    });

    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });

    await page.getByTestId("admin-tab-settings").click();
    // Verify auth-specific content loaded in the settings panel
    await expect(
      page.getByText(/registration|authentication/i).first(),
    ).toBeVisible({ timeout: 10000 });
  });

  test("non-admin user is blocked from admin", async ({ page, browser }) => {
    // Register first user (becomes admin)
    await registerAndReachMain(page, {
      usernamePrefix: "admin",
      setupServerName: "E2E Admin Server",
    });

    // Open new context for second user
    const context2 = await browser.newContext({ ignoreHTTPSErrors: true });
    const page2 = await context2.newPage();
    await registerAndReachMain(page2, { usernamePrefix: "nonadmin" });

    // Second user tries to access admin
    await page2.goto("/admin");
    // Use Playwright's negative assertion instead of catch-to-false
    await expect(
      page2.getByRole("heading", { name: "Admin Dashboard" }),
    ).not.toBeVisible({ timeout: 10000 });

    await context2.close();
  });
});
