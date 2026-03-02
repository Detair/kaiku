import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  ensureGuildSelected,
  openGuildSettings,
} from "./helpers";

test.describe("Permissions & Roles", () => {
  test("create a new role with name", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-roles").click();

    const createBtn = page.getByRole("button", { name: /create.*role/i });
    await expect(createBtn).toBeVisible({ timeout: 10000 });
    await createBtn.click();

    await page.getByTestId("role-name-input").fill("Test Moderator");
    await page.getByTestId("role-save").click();

    await expect(
      page.getByText("Test Moderator").first(),
    ).toBeVisible({ timeout: 10000 });
  });

  test("edit existing role name", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-roles").click();

    // Create a role first
    const createBtn = page.getByRole("button", { name: /create.*role/i });
    await expect(createBtn).toBeVisible({ timeout: 10000 });
    await createBtn.click();

    await page.getByTestId("role-name-input").fill("OldRoleName");
    await page.getByTestId("role-save").click();
    await expect(page.getByText("OldRoleName")).toBeVisible({ timeout: 10000 });

    // Click on the role to edit it
    await page.getByText("OldRoleName").click();
    await page.getByTestId("role-name-input").fill("NewRoleName");
    await page.getByTestId("role-save").click();

    await expect(page.getByText("NewRoleName")).toBeVisible({ timeout: 10000 });
  });

  test("@everyone role hides dangerous permission toggles", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-roles").click();

    // Click on @everyone role
    await page.getByText("@everyone").click();

    // Safe permissions should be visible
    await expect(page.getByText("Send Messages")).toBeVisible({ timeout: 10000 });
    await expect(page.getByText("Create Invite")).toBeVisible({ timeout: 10000 });

    // Dangerous permissions should NOT be visible for @everyone
    // These are all marked forbiddenForEveryone: true in permissionConstants.ts
    await expect(page.getByText("Ban Members")).not.toBeVisible({ timeout: 3000 });
    await expect(page.getByText("Kick Members")).not.toBeVisible({ timeout: 3000 });
    await expect(page.getByText("Manage Server")).not.toBeVisible({ timeout: 3000 });
    await expect(page.getByText("Manage Roles")).not.toBeVisible({ timeout: 3000 });
    await expect(page.getByText("Manage Messages")).not.toBeVisible({ timeout: 3000 });
    await expect(page.getByText("Transfer Ownership")).not.toBeVisible({ timeout: 3000 });
  });

  test("custom role shows all permission toggles including dangerous ones", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-roles").click();

    // Create a custom role
    const createBtn = page.getByRole("button", { name: /create.*role/i });
    await expect(createBtn).toBeVisible({ timeout: 10000 });
    await createBtn.click();

    await page.getByTestId("role-name-input").fill("Full Mod");

    // Custom roles should show ALL permissions including dangerous ones
    await expect(page.getByText("Send Messages")).toBeVisible({ timeout: 10000 });
    await expect(page.getByText("Ban Members")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("Kick Members")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("Manage Server")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("Manage Roles")).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("Manage Messages")).toBeVisible({ timeout: 5000 });
  });

  test("toggle permission on role and save", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-roles").click();

    // Create a role
    const createBtn = page.getByRole("button", { name: /create.*role/i });
    await expect(createBtn).toBeVisible({ timeout: 10000 });
    await createBtn.click();

    await page.getByTestId("role-name-input").fill("Perm Test Role");

    // Toggle Manage Messages permission
    const manageMessagesLabel = page.locator("label").filter({ hasText: "Manage Messages" });
    await expect(manageMessagesLabel).toBeVisible({ timeout: 5000 });
    const checkbox = manageMessagesLabel.locator('input[type="checkbox"]');
    await checkbox.check();
    expect(await checkbox.isChecked()).toBe(true);

    await page.getByTestId("role-save").click();
    await expect(page.getByText("Perm Test Role")).toBeVisible({ timeout: 10000 });
  });

  test("members tab shows guild members", async ({ page }) => {
    const { username } = await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-members").click();
    await expect(page.getByText(username)).toBeVisible({ timeout: 10000 });
  });
});
